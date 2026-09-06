//! Undo/redo. A change to the document is expressed as a list of `Action`s
//! applied atomically as one "level". Applying an action returns the inverse
//! action; the inverses are stored in the undo stack. Undoing a level applies
//! its stored actions and pushes the resulting inverses onto the redo stack
//! (and vice versa).
use {
    crate::document::{
        Document,
        Edge,
        EdgeId,
        Layer,
        LayerId,
        Node,
        NodeId,
    },
};

/// Changes to the same target of the same kind within this window are coalesced
/// into one undo level (e.g. typing).
pub const COALESCE_MS: f64 = 200.;

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    NodeCreate {
        node: Node,
        index: Option<usize>,
    },
    NodeDelete(NodeId),
    /// Replace the node with the same id.
    NodeModify(Node),
    EdgeCreate {
        edge: Edge,
        index: Option<usize>,
    },
    EdgeDelete(EdgeId),
    EdgeModify(Edge),
    LayerCreate {
        layer: Layer,
        index: Option<usize>,
    },
    LayerDelete(LayerId),
    LayerModify(Layer),
    SelectLayer(Option<LayerId>),
}

/// Identifies the kind of modification, for coalescing successive edits (e.g.
/// typing into the same text field).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoalesceKey {
    pub target: String,
    pub field: String,
}

/// Apply an action to the document, returning the inverse action. Actions that
/// don't apply (missing ids) are no-ops returning an inverse no-op.
pub fn apply_action(doc: &mut Document, action: Action) -> Option<Action> {
    match action {
        Action::NodeCreate { node, index } => {
            let id = node.id.clone();
            let index = index.unwrap_or(doc.nodes.len()).min(doc.nodes.len());
            doc.nodes.insert(index, node);
            return Some(Action::NodeDelete(id));
        },
        Action::NodeDelete(id) => {
            let index = doc.nodes.iter().position(|n| n.id == id)?;
            let node = doc.nodes.remove(index);
            return Some(Action::NodeCreate {
                node: node,
                index: Some(index),
            });
        },
        Action::NodeModify(node) => {
            let slot = doc.node_mut(&node.id)?;
            let old = std::mem::replace(slot, node);
            return Some(Action::NodeModify(old));
        },
        Action::EdgeCreate { edge, index } => {
            let id = edge.id.clone();
            let index = index.unwrap_or(doc.edges.len()).min(doc.edges.len());
            doc.edges.insert(index, edge);
            return Some(Action::EdgeDelete(id));
        },
        Action::EdgeDelete(id) => {
            let index = doc.edges.iter().position(|e| e.id == id)?;
            let edge = doc.edges.remove(index);
            return Some(Action::EdgeCreate {
                edge: edge,
                index: Some(index),
            });
        },
        Action::EdgeModify(edge) => {
            let slot = doc.edge_mut(&edge.id)?;
            let old = std::mem::replace(slot, edge);
            return Some(Action::EdgeModify(old));
        },
        Action::LayerCreate { layer, index } => {
            let id = layer.id.clone();
            let index = index.unwrap_or(doc.layers.len()).min(doc.layers.len());
            doc.layers.insert(index, layer);
            return Some(Action::LayerDelete(id));
        },
        Action::LayerDelete(id) => {
            let index = doc.layers.iter().position(|l| l.id == id)?;
            let layer = doc.layers.remove(index);
            return Some(Action::LayerCreate {
                layer: layer,
                index: Some(index),
            });
        },
        Action::LayerModify(layer) => {
            let slot = doc.layer_mut(&layer.id)?;
            let old = std::mem::replace(slot, layer);
            return Some(Action::LayerModify(old));
        },
        Action::SelectLayer(layer) => {
            let old = std::mem::replace(&mut doc.selected_layer, layer);
            return Some(Action::SelectLayer(old));
        },
    }
}

#[derive(Clone, Debug)]
pub struct Level {
    /// Actions to apply to revert (or redo) this level, in application order.
    pub actions: Vec<Action>,
    pub time_ms: f64,
    pub key: Option<CoalesceKey>,
}

#[derive(Default, Debug)]
pub struct History {
    pub undo: Vec<Level>,
    pub redo: Vec<Level>,
}

impl History {
    /// Apply a change (a list of actions, atomically) and record it. `key`
    /// enables coalescing with the previous level if it has the same key and
    /// happened recently.
    pub fn commit(&mut self, doc: &mut Document, actions: Vec<Action>, now_ms: f64, key: Option<CoalesceKey>) {
        let mut inverses = vec![];
        for a in actions {
            if let Some(inv) = apply_action(doc, a) {
                inverses.push(inv);
            }
        }
        inverses.reverse();
        self.redo.clear();
        if let Some(key) = &key {
            if let Some(top) = self.undo.last_mut() {
                if top.key.as_ref() == Some(key) && now_ms - top.time_ms < COALESCE_MS {
                    // The top level already restores the state before both edits; drop the new
                    // inverse and extend the window.
                    top.time_ms = now_ms;
                    return;
                }
            }
        }
        if inverses.is_empty() {
            return;
        }
        self.undo.push(Level {
            actions: inverses,
            time_ms: now_ms,
            key: key,
        });
    }

    pub fn can_undo(&self) -> bool {
        return !self.undo.is_empty();
    }

    pub fn can_redo(&self) -> bool {
        return !self.redo.is_empty();
    }

    pub fn undo(&mut self, doc: &mut Document, now_ms: f64) -> bool {
        let Some(level) = self.undo.pop() else {
            return false;
        };
        let inverse = Self::apply_level(doc, level, now_ms);
        self.redo.push(inverse);
        return true;
    }

    pub fn redo(&mut self, doc: &mut Document, now_ms: f64) -> bool {
        let Some(level) = self.redo.pop() else {
            return false;
        };
        let inverse = Self::apply_level(doc, level, now_ms);
        self.undo.push(inverse);
        return true;
    }

    fn apply_level(doc: &mut Document, level: Level, now_ms: f64) -> Level {
        let mut inverses = vec![];
        for a in level.actions {
            if let Some(inv) = apply_action(doc, a) {
                inverses.push(inv);
            }
        }
        inverses.reverse();
        return Level {
            actions: inverses,
            time_ms: now_ms,
            // Applied levels are never coalesced with
            key: None,
        };
    }
}

/// Build the actions for deleting a node along with everything that refers to
/// it (edges, parent references).
pub fn delete_node_actions(doc: &Document, id: &NodeId) -> Vec<Action> {
    let mut out = vec![];
    for e in &doc.edges {
        if &e.source == id || &e.dest == id {
            out.push(Action::EdgeDelete(e.id.clone()));
        }
    }
    for n in &doc.nodes {
        if n.parents.contains(id) {
            let mut n = n.clone();
            n.parents.retain(|p| p != id);
            out.push(Action::NodeModify(n));
        }
    }
    out.push(Action::NodeDelete(id.clone()));
    return out;
}

/// Build the actions for deleting a layer along with all references to it.
pub fn delete_layer_actions(doc: &Document, id: &LayerId) -> Vec<Action> {
    let mut out = vec![];
    for n in &doc.nodes {
        if n.layers.contains(id) {
            let mut n = n.clone();
            n.layers.retain(|l| l != id);
            out.push(Action::NodeModify(n));
        }
    }
    for e in &doc.edges {
        if e.layer.as_ref() == Some(id) {
            let mut e = e.clone();
            e.layer = None;
            out.push(Action::EdgeModify(e));
        }
    }
    if doc.selected_layer.as_ref() == Some(id) {
        out.push(Action::SelectLayer(None));
    }
    out.push(Action::LayerDelete(id.clone()));
    return out;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str) -> Node {
        return Node {
            id: NodeId(id.to_string()),
            text: id.to_string(),
            layers: vec![],
            parents: vec![],
        };
    }

    #[test]
    fn undo_redo_roundtrip() {
        let mut doc = Document::default();
        let mut h = History::default();
        h.commit(&mut doc, vec![Action::NodeCreate {
            node: node("a"),
            index: None,
        }], 0., None);
        h.commit(&mut doc, vec![Action::NodeCreate {
            node: node("b"),
            index: None,
        }], 1000., None);
        let e = Edge {
            id: EdgeId("e".into()),
            text: "".into(),
            source: NodeId("a".into()),
            dest: NodeId("b".into()),
            layer: None,
        };
        h.commit(&mut doc, vec![Action::EdgeCreate {
            edge: e,
            index: None,
        }], 2000., None);
        let full = doc.clone();
        let actions = delete_node_actions(&doc, &NodeId("a".into()));
        h.commit(&mut doc, actions, 3000., None);
        assert_eq!(doc.nodes.len(), 1);
        assert_eq!(doc.edges.len(), 0);
        assert!(h.undo(&mut doc, 4000.));
        assert_eq!(doc, full);
        assert_eq!(doc.nodes[0].id.0, "a");
        assert!(h.redo(&mut doc, 5000.));
        assert_eq!(doc.nodes.len(), 1);
        assert!(h.undo(&mut doc, 6000.));
        assert!(h.undo(&mut doc, 6000.));
        assert_eq!(doc.edges.len(), 0);
        assert_eq!(doc.nodes.len(), 2);
    }

    #[test]
    fn coalesce() {
        let mut doc = Document::default();
        let mut h = History::default();
        h.commit(&mut doc, vec![Action::NodeCreate {
            node: node("a"),
            index: None,
        }], 0., None);
        let key = CoalesceKey {
            target: "a".into(),
            field: "text".into(),
        };
        for (i, t) in ["x", "xy", "xyz"].iter().enumerate() {
            let mut n = doc.node(&NodeId("a".into())).unwrap().clone();
            n.text = t.to_string();
            h.commit(&mut doc, vec![Action::NodeModify(n)], 1000. + i as f64 * 50., Some(key.clone()));
        }
        assert_eq!(h.undo.len(), 2);
        h.undo(&mut doc, 2000.);
        assert_eq!(doc.node(&NodeId("a".into())).unwrap().text, "a");
        h.redo(&mut doc, 2000.);
        assert_eq!(doc.node(&NodeId("a".into())).unwrap().text, "xyz");
    }
}
