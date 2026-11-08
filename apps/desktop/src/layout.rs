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

/// A side of a pane: where a dragged tab's panes are dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

/// How much of a pane's width or height, from each side, is a drop zone.
pub const EDGE_BAND: f32 = 0.28;

/// The drop zone under a point at (`fx`, `fy`) inside a pane (both 0..1,
/// from the top left): the nearest side when the point is within
/// [`EDGE_BAND`] of it, nothing in the middle.
pub fn edge_at(fx: f32, fy: f32) -> Option<Edge> {
    if !(fx.is_finite() && fy.is_finite()) {
        return None;
    }
    let (fx, fy) = (fx.clamp(0.0, 1.0), fy.clamp(0.0, 1.0));
    let candidates = [
        (fx, Edge::Left),
        (1.0 - fx, Edge::Right),
        (fy, Edge::Top),
        (1.0 - fy, Edge::Bottom),
    ];
    candidates
        .into_iter()
        .filter(|(d, _)| *d < EDGE_BAND)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, e)| e)
}

/// Move the item at `from` so it lands in gap `slot` (0 = before the first,
/// `len` = after the last), as when a tab is dropped between two others.
/// Returns its new index, or `None` when nothing moved.
pub fn reorder<T>(items: &mut Vec<T>, from: usize, slot: usize) -> Option<usize> {
    if from >= items.len() {
        return None;
    }
    let slot = slot.min(items.len());
    // Dropping into the gap right before or after itself changes nothing.
    if slot == from || slot == from + 1 {
        return None;
    }
    let item = items.remove(from);
    let to = if slot > from { slot - 1 } else { slot };
    items.insert(to, item);
    Some(to)
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

    /// Put a whole subtree beside pane `target`, on its `edge` (a dragged
    /// tab's panes become a split next to it). Gives the subtree back if
    /// `target` is not in this tree.
    pub fn insert_at(
        &mut self,
        target: u64,
        edge: Edge,
        subtree: Node<P>,
        split_id: u64,
    ) -> Result<(), Node<P>> {
        match self {
            Node::Leaf(p) if p.pane_id() == target => {
                let old = std::mem::replace(self, Node::Empty);
                let (first, second) = match edge {
                    Edge::Left | Edge::Top => (subtree, old),
                    Edge::Right | Edge::Bottom => (old, subtree),
                };
                *self = Node::Split {
                    id: split_id,
                    vertical: matches!(edge, Edge::Top | Edge::Bottom),
                    ratio: 0.5,
                    a: Box::new(first),
                    b: Box::new(second),
                };
                Ok(())
            }
            Node::Leaf(_) | Node::Empty => Err(subtree),
            Node::Split { a, b, .. } => match a.insert_at(target, edge, subtree, split_id) {
                Ok(()) => Ok(()),
                Err(sub) => b.insert_at(target, edge, sub, split_id),
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

// ---- grids ----------------------------------------------------------------

/// Panes per row for a grid of `n` panes: as square as possible
/// (`ceil(sqrt(n))` columns), rows differing by at most one pane, fuller
/// rows on top. `[]` for none.
pub fn grid_shape(n: usize) -> Vec<usize> {
    if n == 0 {
        return Vec::new();
    }
    let cols = (1..=n).find(|c| c * c >= n).unwrap_or(n);
    let rows = n.div_ceil(cols);
    let (base, extra) = (n / rows, n % rows);
    (0..rows).map(|r| base + usize::from(r < extra)).collect()
}

/// A tree that tiles `panes` in a grid (see [`grid_shape`]): rows stacked
/// top to bottom, panes side by side in each row, all the same size within
/// a row and every row the same height. `split_id` gives each split a
/// fresh id. None when `panes` is empty.
pub fn grid_tree<P>(panes: Vec<P>, split_id: &mut dyn FnMut() -> u64) -> Option<Node<P>> {
    let shape = grid_shape(panes.len());
    let mut it = panes.into_iter();
    let rows: Vec<Node<P>> = shape
        .iter()
        .filter_map(|&k| {
            chain(
                it.by_ref().take(k).map(Node::Leaf).collect(),
                false,
                split_id,
            )
        })
        .collect();
    chain(rows, true, split_id)
}

/// `nodes` side by side (or stacked when `vertical`) with equal shares.
fn chain<P>(
    nodes: Vec<Node<P>>,
    vertical: bool,
    split_id: &mut dyn FnMut() -> u64,
) -> Option<Node<P>> {
    let mut rev = nodes.into_iter().rev();
    let mut acc = rev.next()?;
    let mut count = 1usize;
    for n in rev {
        count += 1;
        acc = Node::Split {
            id: split_id(),
            vertical,
            // The first of `count` equal parts.
            ratio: 1.0 / count as f32,
            a: Box::new(n),
            b: Box::new(acc),
        };
    }
    Some(acc)
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
    /// Terminal zoom in percent (absent = 100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<u32>,
}

/// Where a window was, in screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SavedBounds {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    #[serde(default)]
    pub maximized: bool,
}

impl SavedBounds {
    /// Bounds that are safe to open a window with: finite, not tiny, not
    /// absurdly large, and at least partly on screen. `screens` is the
    /// rectangle (left, top, right, bottom) covering every display: a
    /// monitor that has been unplugged since must not strand the window.
    pub fn usable(self, screens: Option<(f32, f32, f32, f32)>) -> Option<Self> {
        let finite = [self.x, self.y, self.w, self.h]
            .iter()
            .all(|v| v.is_finite());
        if !finite || self.w < 320.0 || self.h < 240.0 || self.w > 20_000.0 || self.h > 20_000.0 {
            return None;
        }
        if let Some((l, t, r, b)) = screens
            && (self.x > r - 80.0
                || self.y > b - 40.0
                || self.x + self.w < l + 80.0
                || self.y + self.h < t + 40.0)
        {
            return None;
        }
        Some(self)
    }
}

/// A second window and its tabs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedWindow {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<SavedBounds>,
    pub tabs: Vec<SavedTab>,
    #[serde(default)]
    pub active: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedSession {
    /// The first window's tabs (older saves have only these).
    pub tabs: Vec<SavedTab>,
    #[serde(default)]
    pub active: Option<usize>,
    /// The first window's position and size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<SavedBounds>,
    /// More windows, each with its own tabs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub windows: Vec<SavedWindow>,
}

pub const TABS_KEY: &str = "desktop.tabs";

impl SavedSession {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Parse, dropping tabs that are unusable (no host to reconnect to).
    pub fn from_json(json: &str) -> Self {
        let mut s: Self = serde_json::from_str(json).unwrap_or_default();
        s.tabs.retain(|t| t.root.is_usable());
        if s.active.is_some_and(|a| a >= s.tabs.len()) {
            s.active = s.tabs.len().checked_sub(1);
        }
        for w in &mut s.windows {
            w.tabs.retain(|t| t.root.is_usable());
            if w.active.is_some_and(|a| a >= w.tabs.len()) {
                w.active = w.tabs.len().checked_sub(1);
            }
        }
        s.windows.retain(|w| !w.tabs.is_empty());
        s
    }
}

impl SavedNode {
    pub fn is_usable(&self) -> bool {
        match self {
            SavedNode::Pane(p) => p.profile.is_some() || p.quick.is_some(),
            SavedNode::Split { a, b, .. } => a.is_usable() && b.is_usable(),
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
                    zoom: Some(120),
                },
                SavedTab {
                    kind: SavedKind::Files,
                    root: SavedNode::Pane(SavedPane::default()),
                    zoom: None,
                },
            ],
            active: Some(1),
            ..Default::default()
        };
        let back = SavedSession::from_json(&s.to_json());
        // The unusable tab is dropped and `active` clamped.
        assert_eq!(back.tabs.len(), 1);
        assert_eq!(back.active, Some(0));
        assert_eq!(back.tabs[0].root, tree);
        assert_eq!(back.tabs[0].zoom, Some(120));
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
    fn grid_shapes_are_even_and_square() {
        assert_eq!(grid_shape(0), Vec::<usize>::new());
        assert_eq!(grid_shape(1), vec![1]);
        assert_eq!(grid_shape(2), vec![2]);
        assert_eq!(grid_shape(3), vec![2, 1]);
        assert_eq!(grid_shape(4), vec![2, 2]);
        assert_eq!(grid_shape(5), vec![3, 2]);
        assert_eq!(grid_shape(6), vec![3, 3]);
        assert_eq!(grid_shape(7), vec![3, 2, 2]);
        assert_eq!(grid_shape(9), vec![3, 3, 3]);
        assert_eq!(grid_shape(10), vec![4, 3, 3]);
        assert_eq!(grid_shape(12), vec![4, 4, 4]);
        for n in 1..200 {
            let s = grid_shape(n);
            assert_eq!(s.iter().sum::<usize>(), n);
            let (min, max) = (s.iter().min(), s.iter().max());
            assert!(max.zip(min).is_some_and(|(a, b)| a - b <= 1), "{n}: {s:?}");
            // Never more rows than columns.
            assert!(s.len() <= s[0], "{n}: {s:?}");
        }
    }

    #[test]
    fn grid_tree_tiles_evenly() {
        let mut next = 100;
        let mut ids_fn = || {
            next += 1;
            next
        };
        assert!(grid_tree(Vec::<T>::new(), &mut ids_fn).is_none());
        let single = grid_tree(vec![T(1)], &mut ids_fn);
        assert!(matches!(single, Some(Node::Leaf(T(1)))));
        let n = grid_tree((1..=5).map(T).collect(), &mut ids_fn).unwrap_or(Node::Empty);
        assert_eq!(ids(&n), vec![1, 2, 3, 4, 5]);
        let r = n.rects();
        let get = |id: u64| {
            r.iter()
                .find(|(i, _)| *i == id)
                .map(|(_, r)| *r)
                .unwrap_or_default()
        };
        let close = |a: f32, b: f32| (a - b).abs() < 1e-5;
        // Row one: three thirds; row two: two halves; rows half the height.
        for (id, x, w) in [
            (1, 0.0, 1.0 / 3.0),
            (2, 1.0 / 3.0, 1.0 / 3.0),
            (3, 2.0 / 3.0, 1.0 / 3.0),
        ] {
            let (rx, ry, rw, rh) = get(id);
            assert!(
                close(rx, x) && close(rw, w) && close(ry, 0.0) && close(rh, 0.5),
                "{id}"
            );
        }
        for (id, x) in [(4, 0.0), (5, 0.5)] {
            let (rx, ry, rw, rh) = get(id);
            assert!(
                close(rx, x) && close(rw, 0.5) && close(ry, 0.5) && close(rh, 0.5),
                "{id}"
            );
        }
        // Split ids are unique; navigation works across rows.
        assert_eq!(n.neighbor(1, Dir::Down), Some(4));
        assert!(n.neighbor(5, Dir::Up).is_some());
        // Twelve panes: 4 × 3, every pane a twelfth.
        let n = grid_tree((1..=12).map(T).collect(), &mut ids_fn).unwrap_or(Node::Empty);
        assert!(
            n.rects()
                .iter()
                .all(|(_, (_, _, w, h))| close(w * h, 1.0 / 12.0))
        );
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
                zoom: None,
            }],
            active: Some(0),
            ..Default::default()
        };
        let back = SavedSession::from_json(&s.to_json());
        assert_eq!(back.tabs[0].root, SavedNode::Pane(p));
    }

    #[test]
    fn dropping_a_tab_on_a_pane_edge_makes_a_split() {
        let mut n = Node::Leaf(T(1));
        let _ = n.split(1, T(2), false, 100); // 1 | 2
        // A dragged tab with two panes goes to the left of pane 2.
        let mut sub = Node::Leaf(T(10));
        let _ = sub.split(10, T(11), true, 101); // 10 / 11
        assert!(n.insert_at(2, Edge::Left, sub, 102).is_ok());
        assert_eq!(ids(&n), vec![1, 10, 11, 2]);
        assert_eq!(n.count(), 4);
        // 10 and 11 stay stacked; the group sits between 1 and 2.
        assert_eq!(n.neighbor(10, Dir::Left), Some(1));
        assert_eq!(n.neighbor(11, Dir::Up), Some(10));
        assert_eq!(n.neighbor(10, Dir::Right), Some(2));
        // Bottom edge: below pane 1.
        assert!(n.insert_at(1, Edge::Bottom, Node::Leaf(T(20)), 103).is_ok());
        assert_eq!(n.neighbor(1, Dir::Down), Some(20));
        // Top edge puts it above.
        assert!(n.insert_at(2, Edge::Top, Node::Leaf(T(21)), 104).is_ok());
        assert_eq!(n.neighbor(2, Dir::Up), Some(21));
        // Right edge puts it after.
        assert!(n.insert_at(2, Edge::Right, Node::Leaf(T(22)), 105).is_ok());
        assert_eq!(n.neighbor(2, Dir::Right), Some(22));
        // An unknown target gives the subtree back untouched.
        let back = n.insert_at(99, Edge::Left, Node::Leaf(T(30)), 106);
        assert!(matches!(back, Err(Node::Leaf(T(30)))));
        // The panes can still be closed one by one afterwards.
        assert_eq!(n.remove(10), Some(T(10)));
        assert_eq!(n.count(), 6);
    }

    #[test]
    fn drop_zones_are_the_pane_edges() {
        assert_eq!(edge_at(0.05, 0.5), Some(Edge::Left));
        assert_eq!(edge_at(0.95, 0.5), Some(Edge::Right));
        assert_eq!(edge_at(0.5, 0.05), Some(Edge::Top));
        assert_eq!(edge_at(0.5, 0.97), Some(Edge::Bottom));
        assert_eq!(edge_at(0.5, 0.5), None);
        // In a corner the nearer side wins.
        assert_eq!(edge_at(0.10, 0.05), Some(Edge::Top));
        assert_eq!(edge_at(0.04, 0.2), Some(Edge::Left));
        // Just outside the band, and the extremes.
        assert_eq!(edge_at(EDGE_BAND + 0.01, 0.5), None);
        assert_eq!(edge_at(0.0, 0.0), Some(Edge::Left));
        assert_eq!(edge_at(-3.0, 0.5), Some(Edge::Left));
        assert_eq!(edge_at(f32::NAN, 0.5), None);
    }

    #[test]
    fn tabs_reorder_into_gaps() {
        let mk = || vec!['a', 'b', 'c', 'd'];
        // Gap numbers: 0 a 1 b 2 c 3 d 4.
        let mut v = mk();
        assert_eq!(reorder(&mut v, 0, 3), Some(2));
        assert_eq!(v, ['b', 'c', 'a', 'd']);
        let mut v = mk();
        assert_eq!(reorder(&mut v, 3, 0), Some(0));
        assert_eq!(v, ['d', 'a', 'b', 'c']);
        let mut v = mk();
        assert_eq!(reorder(&mut v, 1, 4), Some(3));
        assert_eq!(v, ['a', 'c', 'd', 'b']);
        // Dropping next to itself, or on a missing tab, changes nothing.
        for (from, slot) in [(1, 1), (1, 2), (0, 0), (3, 4), (9, 0)] {
            let mut v = mk();
            assert_eq!(reorder(&mut v, from, slot), None, "{from}->{slot}");
            assert_eq!(v, mk());
        }
        // A slot past the end counts as the end.
        let mut v = mk();
        assert_eq!(reorder(&mut v, 0, 99), Some(3));
        assert_eq!(v, ['b', 'c', 'd', 'a']);
    }

    #[test]
    fn windows_round_trip_and_bad_data_is_dropped() {
        let tab = |q: &str| SavedTab {
            kind: SavedKind::Terminal,
            root: SavedNode::Pane(SavedPane {
                quick: Some(q.into()),
                ..Default::default()
            }),
            zoom: None,
        };
        let s = SavedSession {
            tabs: vec![tab("a@b")],
            active: Some(0),
            bounds: Some(SavedBounds {
                x: 10.0,
                y: 20.0,
                w: 1200.0,
                h: 800.0,
                maximized: false,
            }),
            windows: vec![
                SavedWindow {
                    bounds: Some(SavedBounds {
                        x: 900.0,
                        y: 40.0,
                        w: 700.0,
                        h: 500.0,
                        maximized: true,
                    }),
                    tabs: vec![tab("c@d"), tab("e@f")],
                    active: Some(9),
                },
                // A window whose tabs are all unusable disappears.
                SavedWindow {
                    bounds: None,
                    tabs: vec![SavedTab {
                        kind: SavedKind::Files,
                        root: SavedNode::Pane(SavedPane::default()),
                        zoom: None,
                    }],
                    active: None,
                },
            ],
        };
        let back = SavedSession::from_json(&s.to_json());
        assert_eq!(back.bounds, s.bounds);
        assert_eq!(back.windows.len(), 1);
        assert_eq!(back.windows[0].tabs.len(), 2);
        assert_eq!(back.windows[0].active, Some(1), "active is clamped");
        assert!(back.windows[0].bounds.is_some_and(|b| b.maximized));
        // Saves from before windows existed still load.
        let old = r#"{"tabs":[{"kind":"terminal","root":{"t":"pane","quick":"a@b"}}],"active":0}"#;
        let o = SavedSession::from_json(old);
        assert_eq!((o.tabs.len(), o.windows.len(), o.bounds), (1, 0, None));
    }

    #[test]
    fn saved_bounds_must_be_usable() {
        let b = |x: f32, y: f32, w: f32, h: f32| SavedBounds {
            x,
            y,
            w,
            h,
            maximized: false,
        };
        let screen = Some((0.0, 0.0, 1920.0, 1080.0));
        assert!(b(100.0, 100.0, 1000.0, 700.0).usable(screen).is_some());
        // Negative origins are fine (a monitor to the left).
        assert!(b(-1200.0, 50.0, 1000.0, 700.0).usable(None).is_some());
        assert!(b(100.0, 100.0, 50.0, 700.0).usable(screen).is_none());
        assert!(b(f32::NAN, 0.0, 800.0, 600.0).usable(None).is_none());
        // Off every screen: the monitor is gone.
        assert!(b(4000.0, 100.0, 800.0, 600.0).usable(screen).is_none());
        assert!(b(-5000.0, 100.0, 800.0, 600.0).usable(screen).is_none());
        // With a monitor to the left the union starts at a negative x.
        let two = Some((-1920.0, 0.0, 1920.0, 1080.0));
        assert!(b(-1800.0, 100.0, 800.0, 600.0).usable(two).is_some());
    }
}
