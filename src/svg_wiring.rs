use resvg::{tiny_skia, usvg};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fmt::Write,
};

const MAX_NETS: usize = 64;
const MAX_SEGMENTS: usize = 4096;
const CYEAM_METADATA: &str = "cyeam-wire-routes";
const CYEAM_WIRE: &str = "data-cyeam-wire";

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

#[derive(Deserialize)]
struct CyeamRoute {
    wires: Vec<usize>,
    pins: Vec<CyeamPin>,
}

#[derive(Deserialize)]
struct CyeamPin {
    part: String,
    pin: String,
    at: [f32; 2],
}

pub(crate) fn parse_xml(source: &str) -> Option<roxmltree::Document<'_>> {
    if !(source.contains("data-wire-hit") || source.contains(CYEAM_METADATA)) {
        return None;
    }
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    roxmltree::Document::parse_with_options(source, options).ok()
}

fn alloy_metadata<'a, 'input>(
    xml: &'a roxmltree::Document<'input>,
) -> Option<roxmltree::Node<'a, 'input>> {
    xml.root_element()
        .children()
        .find(|node| node.has_tag_name("metadata") && node.attribute("id") == Some("wiring"))
}

fn cyeam_layer<'a, 'input>(
    xml: &'a roxmltree::Document<'input>,
) -> Option<(roxmltree::Node<'a, 'input>, roxmltree::Node<'a, 'input>)> {
    let metadata = xml.descendants().find(|node| {
        node.has_tag_name("metadata") && node.attribute("id") == Some(CYEAM_METADATA)
    })?;
    let drawing = metadata.ancestors().find(|node| node.has_tag_name("svg"))?;
    let wires = drawing
        .descendants()
        .find(|node| node.has_tag_name("g") && node.attribute("id") == Some("wires"))?;
    Some((metadata, wires))
}

/// Give generated strokes renderer IDs so hit geometry uses the same transforms as the image.
pub(crate) fn cyeam_stroke_ids(source: &str, xml: &roxmltree::Document<'_>) -> Option<String> {
    if alloy_metadata(xml).is_some() {
        return None;
    }
    let (_, layer) = cyeam_layer(xml)?;
    let mut ids: HashSet<_> = xml
        .descendants()
        .filter_map(|node| node.attribute("id").map(str::to_owned))
        .collect();
    let mut patched = String::new();
    let mut offset = 0;
    for node in layer
        .descendants()
        .filter(|node| node.is_element() && !node.has_attribute("id"))
    {
        let Some(attribute) = node.attribute_node(CYEAM_WIRE) else {
            continue;
        };
        let index = attribute.value().parse::<usize>().ok()?;
        let mut id = format!("alloy-cyeam-wire-{index}");
        while !ids.insert(id.clone()) {
            id.push('_');
        }
        let end = attribute.range().end;
        patched.push_str(&source[offset..end]);
        write!(patched, " id=\"{id}\"").ok()?;
        offset = end;
    }
    if offset == 0 {
        return None;
    }
    patched.push_str(&source[offset..]);
    Some(patched)
}

impl Diagram {
    pub(crate) fn parse(xml: &roxmltree::Document<'_>, tree: &usvg::Tree) -> Option<Self> {
        let Some(metadata) = alloy_metadata(xml) else {
            return Self::from_cyeam(xml, tree);
        };
        let mut diagram: Self = serde_json::from_str(metadata.text()?).ok()?;
        if !is_identifier(&diagram.layer) || diagram.nets.len() > MAX_NETS {
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
            diagram.nets[index].add_stroke(tree, node.attribute("id")?, &mut count)?;
        }
        diagram.nets.retain(|net| !net.segments.is_empty());
        (!diagram.nets.is_empty()).then_some(diagram)
    }

    fn from_cyeam(xml: &roxmltree::Document<'_>, tree: &usvg::Tree) -> Option<Self> {
        let (metadata, layer) = cyeam_layer(xml)?;
        let layer_id = layer.attribute("id")?;
        let transform = tree.node_by_id(layer_id)?.abs_transform();
        let routes: Vec<CyeamRoute> = serde_json::from_str(metadata.text()?).ok()?;
        if routes.len() > MAX_NETS {
            return None;
        }
        let mut wire_ids = HashMap::new();
        for node in layer
            .descendants()
            .filter(|node| node.has_attribute(CYEAM_WIRE))
        {
            let index = node.attribute(CYEAM_WIRE)?.parse::<usize>().ok()?;
            if wire_ids.insert(index, node.attribute("id")?).is_some() {
                return None;
            }
        }
        let mut nets = Vec::new();
        let mut count = 0;
        let mut assigned = HashSet::new();
        for (index, route) in routes.into_iter().enumerate() {
            if !(2..=32).contains(&route.pins.len()) {
                return None;
            }
            let mut endpoints = Vec::new();
            for pin in route.pins {
                let mut point = tiny_skia::Point::from_xy(pin.at[0], pin.at[1]);
                transform.map_point(&mut point);
                if pin.part.trim().is_empty() || !point.x.is_finite() || !point.y.is_finite() {
                    return None;
                }
                endpoints.push(Endpoint {
                    device: pin.part,
                    pin: pin.pin,
                    x: point.x,
                    y: point.y,
                });
            }
            let first = &endpoints[0];
            let name = if first.pin.is_empty() {
                first.device.clone()
            } else {
                first.pin.clone()
            };
            let mut net = Net {
                id: format!("route-{}", index + 1),
                name,
                endpoints,
                segments: Vec::new(),
            };
            for wire in route.wires {
                if !assigned.insert(wire) {
                    return None;
                }
                net.add_stroke(tree, wire_ids.get(&wire)?, &mut count)?;
            }
            if !net.segments.is_empty() {
                nets.push(net);
            }
        }
        (!nets.is_empty()).then(|| Self {
            layer: layer_id.into(),
            nets,
        })
    }
}

impl Net {
    fn add_stroke(&mut self, tree: &usvg::Tree, id: &str, count: &mut usize) -> Option<()> {
        if let Some(usvg::Node::Path(path)) = tree.node_by_id(id) {
            for segment in path.data().segments() {
                *count += 1;
                if *count > MAX_SEGMENTS {
                    return None;
                }
                self.segments
                    .push(path_segment(segment, path.abs_transform())?);
            }
        }
        Some(())
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
