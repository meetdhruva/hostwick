//! Split-pane trees and the saved tab layout (restore tabs).
//!
//! Pure logic, independent of GPUI: a binary tree whose leaves are panes.
//! Splitting a leaf replaces it with a split of the old and the new pane;
//! closing one collapses its parent into the sibling.

use hostwick_core::ProfileId;
use serde::{Deserialize, Serialize};

/// A pane tree. `P` is the pane payload; every pane has a unique id.
#[derive(Debug)]
pub enum Node<P> {
    /// Transient: only exists while the tree is being restructured.
    Empty,
    Leaf(P),
    Split {
        id: u64,
        /// Children stacked top/bottom (else side by side).
        vertical: bool,
        /// Share of the space that `a` gets (0..1).
        ratio: f32,
        a: Box<Node<P>>,
        b: Box<Node<P>>,
    },
}

/// Ratios are kept inside this range (the UI also keeps a minimum size).
pub const MIN_RATIO: f32 = 0.05;

/// A ratio from outside (saved file, drag) made safe to use.
pub fn clamp_ratio(r: f32) -> f32 {
    if r.is_finite() {
        r.clamp(MIN_RATIO, 1.0 - MIN_RATIO)
    } else {
        0.5
    }
}

pub trait PaneId {
    fn pane_id(&self) -> u64;
}

/// Direction for moving focus between panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// A rectangle in the unit square: x, y, w, h.
pub type Rect = (f32, f32, f32, f32);

impl<P: PaneId> Node<P> {
    pub fn leaves(&self) -> Vec<&P> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect<'a>(&'a self, out: &mut Vec<&'a P>) {
        match self {
            Node::Empty => {}
            Node::Leaf(p) => out.push(p),
            Node::Split { a, b, .. } => {
                a.collect(out);
                b.collect(out);
            }
        }
    }

    pub fn leaves_mut(&mut self) -> Vec<&mut P> {
        let mut out = Vec::new();
        self.collect_mut(&mut out);
        out
    }

    fn collect_mut<'a>(&'a mut self, out: &mut Vec<&'a mut P>) {
        match self {
            Node::Empty => {}
            Node::Leaf(p) => out.push(p),
            Node::Split { a, b, .. } => {
                a.collect_mut(out);
                b.collect_mut(out);
            }
        }
    }

    pub fn find(&self, id: u64) -> Option<&P> {
        self.leaves().into_iter().find(|p| p.pane_id() == id)
    }

    pub fn find_mut(&mut self, id: u64) -> Option<&mut P> {
        self.leaves_mut().into_iter().find(|p| p.pane_id() == id)
    }

    pub fn count(&self) -> usize {
        match self {
            Node::Empty => 0,
            Node::Leaf(_) => 1,
            Node::Split { a, b, .. } => a.count() + b.count(),
        }
    }

    /// Split pane `target`, putting `new` right of it (or below it when
    /// `vertical`). Gives `new` back if `target` is not in the tree.
    pub fn split(&mut self, target: u64, new: P, vertical: bool, split_id: u64) -> Result<(), P> {
        match self {
            Node::Leaf(p) if p.pane_id() == target => {
                let old = std::mem::replace(self, Node::Empty);
                *self = Node::Split {
                    id: split_id,
                    vertical,
                    ratio: 0.5,
                    a: Box::new(old),
                    b: Box::new(Node::Leaf(new)),
                };
                Ok(())
            }
            Node::Leaf(_) | Node::Empty => Err(new),
            Node::Split { a, b, .. } => match a.split(target, new, vertical, split_id) {
                Ok(()) => Ok(()),
                Err(new) => b.split(target, new, vertical, split_id),
            },
        }
    }

    /// Remove pane `id` (never the last one). Returns it.
    pub fn remove(&mut self, id: u64) -> Option<P> {
        let Node::Split { a, b, .. } = self else {
            return None;
        };
        let hit_a = matches!(&**a, Node::Leaf(p) if p.pane_id() == id);
        let hit_b = matches!(&**b, Node::Leaf(p) if p.pane_id() == id);
        if hit_a || hit_b {
            let (gone, keep) = if hit_a { (a, b) } else { (b, a) };
            let keep = std::mem::replace(&mut **keep, Node::Empty);
            let gone = std::mem::replace(&mut **gone, Node::Empty);
            *self = keep;
            return match gone {
                Node::Leaf(p) => Some(p),
                _ => None,
            };
        }
        a.remove(id).or_else(|| b.remove(id))
    }

    /// Set the share of split `split_id`'s first child. False if there is
    /// no such split.
    pub fn set_ratio(&mut self, split_id: u64, ratio: f32) -> bool {
        match self {
            Node::Split { id, ratio: r, .. } if *id == split_id => {
                *r = clamp_ratio(ratio);
                true
            }
            Node::Split { a, b, .. } => {
                a.set_ratio(split_id, ratio) || b.set_ratio(split_id, ratio)
            }
            _ => false,
        }
    }

    /// Move the divider of the innermost `vertical` split around pane `pane`
    /// by `delta` (keyboard resizing). False if there is none.
    pub fn nudge(&mut self, pane: u64, vertical: bool, delta: f32) -> bool {
        let Node::Split {
            vertical: v,
            ratio,
            a,
            b,
            ..
        } = self
        else {
            return false;
        };
        let in_a = a.find(pane).is_some();
        if !in_a && b.find(pane).is_none() {
            return false;
        }
        let inner = if in_a {
            a.nudge(pane, vertical, delta)
        } else {
            b.nudge(pane, vertical, delta)
        };
        if inner {
            return true;
        }
        if *v == vertical {
            *ratio = clamp_ratio(*ratio + delta);
            return true;
        }
        false
    }

    /// Each pane's rectangle (in the unit square).
    pub fn rects(&self) -> Vec<(u64, Rect)> {
        let mut out = Vec::new();
        self.rects_in((0.0, 0.0, 1.0, 1.0), &mut out);
        out
    }

    fn rects_in(&self, r: Rect, out: &mut Vec<(u64, Rect)>) {
        let (x, y, w, h) = r;
        match self {
            Node::Empty => {}
            Node::Leaf(p) => out.push((p.pane_id(), r)),
            Node::Split {
                vertical,
                ratio,
                a,
                b,
                ..
            } => {
                let r = clamp_ratio(*ratio);
                if *vertical {
                    a.rects_in((x, y, w, h * r), out);
                    b.rects_in((x, y + h * r, w, h * (1.0 - r)), out);
                } else {
                    a.rects_in((x, y, w * r, h), out);
                    b.rects_in((x + w * r, y, w * (1.0 - r), h), out);
                }
            }
        }
    }

    /// The pane next to `from` in direction `dir`, if any.
    pub fn neighbor(&self, from: u64, dir: Dir) -> Option<u64> {
        neighbor(&self.rects(), from, dir)
    }
}

/// Pure neighbour search over pane rectangles.
pub fn neighbor(rects: &[(u64, Rect)], from: u64, dir: Dir) -> Option<u64> {
    let (_, (x, y, w, h)) = *rects.iter().find(|(id, _)| *id == from)?;
    let eps = 1e-4;
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    rects
        .iter()
        .filter(|(id, _)| *id != from)
        .filter(|(_, (ox, oy, ow, oh))| {
            let overlap_x = ox < &(x + w - eps) && (ox + ow) > x + eps;
            let overlap_y = oy < &(y + h - eps) && (oy + oh) > y + eps;
            match dir {
                Dir::Left => (ox + ow - x).abs() < eps && overlap_y,
                Dir::Right => (ox - (x + w)).abs() < eps && overlap_y,
                Dir::Up => (oy + oh - y).abs() < eps && overlap_x,
                Dir::Down => (oy - (y + h)).abs() < eps && overlap_x,
            }
        })
        .min_by(|(_, a), (_, b)| {
            let d = |r: &Rect| {
                let (px, py) = (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0);
                (px - cx).powi(2) + (py - cy).powi(2)
            };
            d(a).total_cmp(&d(b))
        })
        .map(|(id, _)| *id)
}

// ---- saved layout ---------------------------------------------------------

/// A pane as saved for "restore tabs".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedPane {
    /// A saved host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<ProfileId>,
    /// Quick-connect text (`user@host:port`) for unsaved hosts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quick: Option<String>,
    /// The pane's id when it was saved, so links between panes survive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// File browser: the saved id of the terminal pane it follows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follows: Option<u64>,
    /// File browser: the remote folder it showed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
}

fn half() -> f32 {
    0.5
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum SavedNode {
    Pane(SavedPane),
    Split {
        vertical: bool,
        /// Share of `a` (older saves have none: even split).
        #[serde(default = "half")]
        ratio: f32,
        a: Box<SavedNode>,
        b: Box<SavedNode>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedKind {
    Terminal,
    Files,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedTab {
    pub kind: SavedKind,
    pub root: SavedNode,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedSession {
    pub tabs: Vec<SavedTab>,
    #[serde(default)]
    pub active: Option<usize>,
}

pub const TABS_KEY: &str = "desktop.tabs";

impl SavedSession {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Parse, dropping tabs that are unusable (no host to reconnect to).
    pub fn from_json(json: &str) -> Self {
        let mut s: Self = serde_json::from_str(json).unwrap_or_default();
        s.tabs.retain(|t| t.root.usable());
        if s.active.is_some_and(|a| a >= s.tabs.len()) {
            s.active = s.tabs.len().checked_sub(1);
        }
        s
    }
}

impl SavedNode {
    fn usable(&self) -> bool {
        match self {
            SavedNode::Pane(p) => p.profile.is_some() || p.quick.is_some(),
            SavedNode::Split { a, b, .. } => a.usable() && b.usable(),
        }
    }

    /// Drop panes whose saved host no longer exists; None if nothing is left.
    pub fn prune(self, exists: &dyn Fn(&SavedPane) -> bool) -> Option<SavedNode> {
        match self {
            SavedNode::Pane(p) => exists(&p).then_some(SavedNode::Pane(p)),
            SavedNode::Split {
                vertical,
                ratio,
                a,
                b,
            } => match (a.prune(exists), b.prune(exists)) {
                (Some(a), Some(b)) => Some(SavedNode::Split {
                    vertical,
                    ratio,
                    a: Box::new(a),
                    b: Box::new(b),
                }),
                (Some(x), None) | (None, Some(x)) => Some(x),
                (None, None) => None,
            },
        }
    }
}

/// Save a live tree.
pub fn save_tree<P: PaneId>(node: &Node<P>, pane: &dyn Fn(&P) -> SavedPane) -> SavedNode {
    match node {
        Node::Empty => SavedNode::Pane(SavedPane::default()),
        Node::Leaf(p) => SavedNode::Pane(pane(p)),
        Node::Split {
            vertical,
            ratio,
            a,
            b,
            ..
        } => SavedNode::Split {
            vertical: *vertical,
            ratio: *ratio,
            a: Box::new(save_tree(a, pane)),
            b: Box::new(save_tree(b, pane)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct T(u64);
    impl PaneId for T {
        fn pane_id(&self) -> u64 {
            self.0
        }
    }

    fn ids(n: &Node<T>) -> Vec<u64> {
        n.leaves().iter().map(|p| p.0).collect()
    }

    #[test]
    fn split_close_and_navigate() {
        let mut n = Node::Leaf(T(1));
        assert!(n.split(1, T(2), false, 100).is_ok()); // 1 | 2
        assert!(n.split(2, T(3), true, 101).is_ok()); // 1 | (2 / 3)
        assert_eq!(ids(&n), vec![1, 2, 3]);
        assert_eq!(n.count(), 3);
        assert!(n.split(9, T(4), true, 102).is_err());
        assert_eq!(n.neighbor(1, Dir::Right), Some(2));
        assert_eq!(n.neighbor(3, Dir::Up), Some(2));
        assert_eq!(n.neighbor(3, Dir::Left), Some(1));
        assert_eq!(n.neighbor(2, Dir::Down), Some(3));
        assert_eq!(n.neighbor(1, Dir::Left), None);
        assert_eq!(n.remove(2), Some(T(2))); // 1 | 3
        assert_eq!(ids(&n), vec![1, 3]);
        assert_eq!(n.neighbor(1, Dir::Right), Some(3));
        assert_eq!(n.remove(1), Some(T(1)));
        assert!(matches!(n, Node::Leaf(T(3))));
        assert_eq!(n.remove(3), None); // never the last pane
    }

    #[test]
    fn saved_layout_round_trips_and_prunes() {
        let mut n = Node::Leaf(T(1));
        let _ = n.split(1, T(2), true, 7);
        let p1 = ProfileId::new();
        assert!(n.set_ratio(7, 0.3));
        let tree = save_tree(&n, &|p| SavedPane {
            profile: (p.0 == 1).then_some(p1),
            quick: (p.0 == 2).then(|| "me@box:2222".to_owned()),
            ..Default::default()
        });
        let s = SavedSession {
            tabs: vec![
                SavedTab {
                    kind: SavedKind::Terminal,
                    root: tree.clone(),
                },
                SavedTab {
                    kind: SavedKind::Files,
                    root: SavedNode::Pane(SavedPane::default()),
                },
            ],
            active: Some(1),
        };
        let back = SavedSession::from_json(&s.to_json());
        // The unusable tab is dropped and `active` clamped.
        assert_eq!(back.tabs.len(), 1);
        assert_eq!(back.active, Some(0));
        assert_eq!(back.tabs[0].root, tree);
        // The deleted host's pane goes; its sibling takes the whole tab.
        let pruned = tree.prune(&|p| p.profile.is_none());
        assert!(matches!(
            pruned,
            Some(SavedNode::Pane(SavedPane { quick: Some(_), .. }))
        ));
        assert_eq!(SavedSession::from_json("garbage"), SavedSession::default());
        // The ratio survives the round trip.
        assert!(
            matches!(back.tabs[0].root, SavedNode::Split { ratio, .. } if (ratio - 0.3).abs() < 1e-6)
        );
    }

    #[test]
    fn ratios_resize_and_old_saves_split_evenly() {
        let mut n = Node::Leaf(T(1));
        let _ = n.split(1, T(2), false, 10); // 1 | 2
        let _ = n.split(2, T(3), true, 11); // 1 | (2 / 3)
        assert!(n.set_ratio(10, 0.25));
        assert!(!n.set_ratio(99, 0.5));
        let r = n.rects();
        let w1 = r.iter().find(|(id, _)| *id == 1).map(|(_, r)| r.2);
        assert_eq!(w1, Some(0.25));
        // Clamped, and NaN is not accepted.
        assert!(n.set_ratio(10, 2.0));
        assert!(n.rects().iter().all(|(_, r)| r.2 > 0.0));
        assert!(n.set_ratio(10, f32::NAN));
        assert_eq!(
            n.rects().iter().find(|(id, _)| *id == 1).map(|(_, r)| r.2),
            Some(0.5)
        );
        // Keyboard: pane 3 is in the vertical split 11.
        assert!(n.nudge(3, true, 0.1));
        assert_eq!(
            n.rects().iter().find(|(id, _)| *id == 2).map(|(_, r)| r.3),
            Some(0.6)
        );
        // Horizontal from pane 3 reaches the outer split.
        assert!(n.nudge(3, false, -0.1));
        assert!(!Node::Leaf(T(5)).nudge(5, true, 0.1));
        // Saves from before ratios existed load as even splits.
        let old = r#"{"tabs":[{"kind":"terminal","root":{"t":"split","vertical":false,
            "a":{"t":"pane","quick":"a@b"},"b":{"t":"pane","quick":"c@d"}}}],"active":0}"#;
        let s = SavedSession::from_json(old);
        assert!(matches!(s.tabs[0].root, SavedNode::Split { ratio, .. } if ratio == 0.5));
    }

    #[test]
    fn links_between_panes_are_saved() {
        let p = SavedPane {
            quick: Some("me@box".into()),
            id: Some(4),
            follows: Some(3),
            dir: Some("/srv".into()),
            ..Default::default()
        };
        let s = SavedSession {
            tabs: vec![SavedTab {
                kind: SavedKind::Files,
                root: SavedNode::Pane(p.clone()),
            }],
            active: Some(0),
        };
        let back = SavedSession::from_json(&s.to_json());
        assert_eq!(back.tabs[0].root, SavedNode::Pane(p));
    }
}
