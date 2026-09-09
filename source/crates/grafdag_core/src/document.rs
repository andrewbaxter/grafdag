use {
    serde::{
        Deserialize,
        Serialize,
    },
    std::collections::HashSet,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DefaultTrue(pub bool);

impl Default for DefaultTrue {
    fn default() -> Self {
        return DefaultTrue(true);
    }
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Document {
    #[serde(default)]
    pub edges: Vec<Edge>,
    #[serde(default)]
    pub flow: crate::layout::Flow,
    #[serde(default)]
    pub layers: Vec<Layer>,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub selected_layer: Option<LayerId>,
}

impl Document {
    pub fn edge(&self, id: &EdgeId) -> Option<&Edge> {
        return self.edges.iter().find(|e| &e.id == id);
    }

    pub fn edge_mut(&mut self, id: &EdgeId) -> Option<&mut Edge> {
        return self.edges.iter_mut().find(|e| &e.id == id);
    }

    pub fn edge_visible(&self, edge: &Edge) -> bool {
        if let Some(l) = &edge.layer {
            if !self.layer_active(l) {
                return false;
            }
        }
        let Some(s) = self.node(&edge.source) else {
            return false;
        };
        let Some(d) = self.node(&edge.dest) else {
            return false;
        };
        return self.node_visible(s) && self.node_visible(d);
    }

    pub fn edges_between<'a>(&'a self, a: &'a NodeId, b: &'a NodeId) -> impl Iterator<Item = &'a Edge> + 'a {
        return self
            .edges
            .iter()
            .filter(move |e| (&e.source == a && &e.dest == b) || (&e.source == b && &e.dest == a));
    }

    pub fn layer(&self, id: &LayerId) -> Option<&Layer> {
        return self.layers.iter().find(|l| &l.id == id);
    }

    pub fn layer_active(&self, id: &LayerId) -> bool {
        return self.layer(id).map(|l| l.active.0).unwrap_or(false);
    }

    pub fn layer_mut(&mut self, id: &LayerId) -> Option<&mut Layer> {
        return self.layers.iter_mut().find(|l| &l.id == id);
    }

    pub fn new_edge_id(&self) -> EdgeId {
        let used: HashSet<&str> = self.edges.iter().map(|e| e.id.0.as_str()).collect();
        let mut i = self.edges.len() + 1;
        loop {
            let candidate = format!("e{}", i);
            if !used.contains(candidate.as_str()) {
                return EdgeId(candidate);
            }
            i += 1;
        }
    }

    pub fn new_layer_id(&self) -> LayerId {
        let used: HashSet<&str> = self.layers.iter().map(|l| l.id.0.as_str()).collect();
        let mut i = self.layers.len() + 1;
        loop {
            let candidate = format!("l{}", i);
            if !used.contains(candidate.as_str()) {
                return LayerId(candidate);
            }
            i += 1;
        }
    }

    pub fn new_node_id(&self) -> NodeId {
        let used: HashSet<&str> = self.nodes.iter().map(|n| n.id.0.as_str()).collect();
        let mut i = self.nodes.len() + 1;
        loop {
            let candidate = format!("n{}", i);
            if !used.contains(candidate.as_str()) {
                return NodeId(candidate);
            }
            i += 1;
        }
    }

    pub fn node(&self, id: &NodeId) -> Option<&Node> {
        return self.nodes.iter().find(|n| &n.id == id);
    }

    pub fn node_mut(&mut self, id: &NodeId) -> Option<&mut Node> {
        return self.nodes.iter_mut().find(|n| &n.id == id);
    }

    pub fn node_visible(&self, node: &Node) -> bool {
        if node.layers.is_empty() {
            return true;
        }
        return node.layers.iter().any(|l| self.layer_active(l));
    }

    pub fn sanitize(&mut self) -> bool {
        let mut changed = false;
        let node_ids: HashSet<NodeId> = self.nodes.iter().map(|n| n.id.clone()).collect();
        let layer_ids: HashSet<LayerId> = self.layers.iter().map(|l| l.id.clone()).collect();
        let before = self.edges.len();
        self.edges.retain(|e| node_ids.contains(&e.source) && node_ids.contains(&e.dest));
        changed |= before != self.edges.len();
        for e in &mut self.edges {
            if let Some(l) = &e.layer {
                if !layer_ids.contains(l) {
                    e.layer = None;
                    changed = true;
                }
            }
        }
        for n in &mut self.nodes {
            let id = n.id.clone();
            let before = n.parents.len();
            n.parents.retain(|p| p != &id && node_ids.contains(p));
            changed |= before != n.parents.len();
            let before = n.layers.len();
            n.layers.retain(|l| layer_ids.contains(l));
            changed |= before != n.layers.len();
        }
        if let Some(l) = &self.selected_layer {
            if !layer_ids.contains(l) {
                self.selected_layer = None;
                changed = true;
            }
        }
        return changed;
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub dest: NodeId,
    pub id: EdgeId,
    #[serde(default)]
    pub layer: Option<LayerId>,
    pub source: NodeId,
    #[serde(default)]
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EdgeId(pub String);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    #[serde(default)]
    pub active: DefaultTrue,
    pub id: LayerId,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LayerId(pub String);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    #[serde(default)]
    pub layers: Vec<LayerId>,
    #[serde(default)]
    pub parents: Vec<NodeId>,
    #[serde(default)]
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub String);
