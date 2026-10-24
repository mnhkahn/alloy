//! Opt-in task monitoring runs independently of rendering, focus and lock state.
mod codex;
mod command;
mod herdr;
mod power;
use serde::Serialize;
use std::{
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Clone, Debug, Default, Serialize)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub title: String,
    pub project: String,
    pub source: String,
}

#[derive(Clone, Default, Serialize)]
pub struct Status {
    pub enabled: bool,
    pub active: bool,
    pub agents: Vec<Agent>,
    pub error: String,
}
#[derive(Default)]
struct State {
    status: Status,
    assertion: Option<power::Assertion>,
    generation: u64,
    stop: bool,
}
#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}
pub struct Monitor {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}
impl Monitor {
    /// UI fixtures must not inspect real sessions or acquire power assertions.
    pub fn new(live: bool) -> Self {
        let shared = Arc::new(Shared::default());
        let worker = live.then(|| {
            let shared = shared.clone();
            thread::spawn(move || monitor(shared))
        });
        Self { shared, worker }
    }
    pub fn status(&self) -> Status {
        self.shared.state.lock().unwrap().status.clone()
    }
    pub fn set_enabled(&self, enabled: bool) -> Status {
        let mut state = self.shared.state.lock().unwrap();
        state.generation += 1;
        state.status.enabled = enabled;
        state.status.active = false;
        // Release synchronously even when a probe is in flight.
        state.assertion = None;
        self.shared.wake.notify_all();
        state.status.clone()
    }
}
impl Drop for Monitor {
    fn drop(&mut self) {
        {
            let mut state = self.shared.state.lock().unwrap();
            state.stop = true;
            state.assertion = None;
            self.shared.wake.notify_all();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn monitor(shared: Arc<Shared>) {
    let mut codex = codex::Codex::default();
    loop {
        let generation = {
            let state = shared.state.lock().unwrap();
            if state.stop {
                return;
            }
            state.generation
        };
        let (herdr, mut errors) = herdr::scan();
        let (codex, codex_errors) = codex.scan();
        errors.extend(codex_errors);
        let agents = merge_agents(herdr, codex);
        let mut state = shared.state.lock().unwrap();
        if state.stop {
            return;
        }
        if state.generation != generation {
            continue;
        }
        if !state.status.enabled || agents.is_empty() {
            state.assertion = None;
        } else if state.assertion.is_none() {
            match power::Assertion::new() {
                Ok(assertion) => state.assertion = Some(assertion),
                Err(error) => errors.push(error),
            }
        }
        state.status.active = state.assertion.is_some();
        state.status.agents = agents;
        state.status.error = errors.join("; ");
        // Use a predicate to avoid lost notifications during rapid off/on toggles.
        let _ = shared
            .wake
            .wait_timeout_while(state, Duration::from_secs(3), |state| {
                !state.stop && state.generation == generation
            })
            .unwrap();
    }
}

fn merge_agents(mut herdr: Vec<Agent>, codex: Vec<Agent>) -> Vec<Agent> {
    for agent in codex {
        if let Some(existing) = herdr.iter_mut().find(|item| item.id == agent.id) {
            // A Codex CLI hosted by Herdr is one agent, seen through two sources.
            existing.title = agent.title;
            existing.source = "Codex / Herdr".into();
        } else {
            herdr.push(agent);
        }
    }
    herdr.sort_by(|a, b| (&a.name, &a.title, &a.id).cmp(&(&b.name, &b.title, &b.id)));
    herdr
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disabling_releases_assertion_and_invalidates_in_flight_scan() {
        let monitor = Monitor::new(false);
        assert!(!monitor.status().enabled);
        monitor.set_enabled(true);
        let generation = monitor.shared.state.lock().unwrap().generation;
        monitor
            .shared
            .state
            .lock()
            .unwrap()
            .status
            .agents
            .push(Agent {
                id: "codex:a".into(),
                ..Agent::default()
            });
        #[cfg(target_os = "macos")]
        {
            monitor.shared.state.lock().unwrap().assertion = Some(power::Assertion::new().unwrap());
        }
        monitor.set_enabled(false);
        let state = monitor.shared.state.lock().unwrap();
        assert!(!state.status.enabled);
        assert!(state.assertion.is_none());
        assert_ne!(state.generation, generation);
        assert_eq!(
            state.status.agents.len(),
            1,
            "turning prevention off must retain the live count/list"
        );
    }
    #[test]
    fn counts_agents_in_one_project_and_merges_shared_session_identity() {
        let codex = ["a", "b"].map(|id| Agent {
            id: format!("codex:{id}"),
            name: "Codex".into(),
            title: format!("Task {id}"),
            project: "/same-project".into(),
            source: "Codex".into(),
        });
        let hosted = Agent {
            name: "Codex CLI".into(),
            title: "terminal title".into(),
            source: "Herdr".into(),
            ..codex[0].clone()
        };
        let agents = merge_agents(vec![hosted], codex.to_vec());
        assert_eq!(agents.len(), 2);
        let hosted = agents.iter().find(|agent| agent.id == "codex:a").unwrap();
        assert_eq!(hosted.name, "Codex CLI");
        assert_eq!(hosted.title, "Task a");
        assert_eq!(hosted.source, "Codex / Herdr");
    }
    #[test]
    #[ignore = "manual smoke test against running local Codex/Herdr tasks"]
    fn live_probes() {
        let (herdr, errors) = herdr::scan();
        assert!(errors.is_empty(), "{errors:?}");
        let (codex, errors) = codex::Codex::default().scan();
        assert!(errors.is_empty(), "{errors:?}");
        eprintln!(
            "Herdr: {}; Codex: {}",
            serde_json::to_string(&herdr).unwrap(),
            serde_json::to_string(&codex).unwrap()
        );
        assert!(
            !codex.is_empty() || !herdr.is_empty(),
            "Start a local task before running this smoke test"
        );
        let monitor = Monitor::new(true);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while monitor.status().agents.is_empty() && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
        let disabled = monitor.status();
        assert!(!disabled.enabled && !disabled.active);
        assert!(
            !disabled.agents.is_empty(),
            "the count/list must work while unchecked"
        );
        monitor.set_enabled(true);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !monitor.status().active && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
        let status = monitor.status();
        assert!(status.active, "{}", status.error);
        let assertions = std::process::Command::new("pmset")
            .args(["-g", "assertions"])
            .output()
            .unwrap();
        let assertions = String::from_utf8_lossy(&assertions.stdout);
        for kind in ["PreventUserIdleSystemSleep", "PreventUserIdleDisplaySleep"] {
            assert!(
                assertions.lines().any(|line| {
                    line.contains(&format!("pid {}(", std::process::id()))
                        && line.contains("Alloy: Codex or Herdr is working")
                        && line.contains(kind)
                }),
                "missing {kind}: {assertions}"
            );
        }
        monitor.set_enabled(false);
        let assertions = std::process::Command::new("pmset")
            .args(["-g", "assertions"])
            .output()
            .unwrap();
        assert!(
            !String::from_utf8_lossy(&assertions.stdout)
                .lines()
                .any(
                    |line| line.contains(&format!("pid {}(", std::process::id()))
                        && line.contains("Alloy: Codex or Herdr is working")
                )
        );
    }
}
