use resvg::{tiny_skia, usvg};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WiringPreview {
    pub muted_asset: String,
    pub nets: Vec<Net>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Net {
    id: String,
    name: String,
    endpoints: Vec<Endpoint>,
    #[serde(default)]
    segments: Vec<Vec<f32>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Endpoint {
    device: String,
    pin: String,
    x: f32,
    y: f32,
}

#[derive(Deserialize)]
pub(crate) struct Diagram {
    pub layer: String,
    pub nets: Vec<Net>,
}

impl Diagram {
    pub(crate) fn parse(xml: &roxmltree::Document<'_>, tree: &usvg::Tree) -> Option<Self> {
        let metadata = xml
            .root_element()
            .children()
            .find(|node| node.has_tag_name("metadata") && node.attribute("id") == Some("wiring"))?;
        let mut diagram: Self = serde_json::from_str(metadata.text()?).ok()?;
        if !is_identifier(&diagram.layer) || diagram.nets.len() > 64 {
            return None;
        }
        let layer = xml.descendants().find(|node| {
            node.has_tag_name("g") && node.attribute("id") == Some(diagram.layer.as_str())
        })?;
        let mut net_indices = HashMap::new();
        for (index, net) in diagram.nets.iter_mut().enumerate() {
            // Click geometry comes from the rendered paths, not the metadata.
            net.segments.clear();
            if !is_identifier(&net.id)
                || net.name.trim().is_empty()
                || net.endpoints.len() < 2
                || net.endpoints.len() > 32
                || net.endpoints.iter().any(|endpoint| {
                    endpoint.device.trim().is_empty()
                        || endpoint.pin.trim().is_empty()
                        || !endpoint.x.is_finite()
                        || !endpoint.y.is_finite()
                })
                || net_indices.insert(net.id.clone(), index).is_some()
            {
                return None;
            }
        }
        let mut count = 0;
        for node in layer
            .descendants()
            .filter(|node| node.has_attribute("data-wire-hit"))
        {
            let index = *net_indices.get(node.attribute("data-net")?)?;
            let Some(usvg::Node::Path(path)) = tree.node_by_id(node.attribute("id")?) else {
                continue;
            };
            for segment in path.data().segments() {
                count += 1;
                if count > 4096 {
                    return None;
                }
                diagram.nets[index]
                    .segments
                    .push(path_segment(segment, path.abs_transform())?);
            }
        }
        diagram.nets.retain(|net| !net.segments.is_empty());
        (!diagram.nets.is_empty()).then_some(diagram)
    }
}

fn is_identifier(value: &str) -> bool {
    value
        .bytes()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == b'_')
        && value.len() <= 80
        && value
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'-' | b'_'))
}

fn path_segment(
    segment: tiny_skia::PathSegment,
    transform: tiny_skia::Transform,
) -> Option<Vec<f32>> {
    use tiny_skia::PathSegment::*;
    let (verb, mut points) = match segment {
        MoveTo(point) => (0.0, vec![point]),
        LineTo(point) => (1.0, vec![point]),
        QuadTo(control, point) => (2.0, vec![point, control]),
        CubicTo(first, second, point) => (3.0, vec![point, first, second]),
        Close => (4.0, vec![]),
    };
    transform.map_points(&mut points);
    points
        .iter()
        .all(|point| point.x.is_finite() && point.y.is_finite())
        .then(|| {
            std::iter::once(verb)
                .chain(points.into_iter().flat_map(|point| [point.x, point.y]))
                .collect()
        })
}
