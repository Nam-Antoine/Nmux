use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PaneId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    Right,
    Down,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    Right,
    Up,
    Down,
}

pub const MIN_RATIO: f32 = 0.1;
pub const MAX_RATIO: f32 = 0.9;

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const UNIT: Rect = Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 };
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Node {
    Leaf {
        pane: PaneId,
    },
    Split {
        dir: Dir,
        ratio: f32,
        a: Box<Node>,
        b: Box<Node>,
    },
}

const EPS: f32 = 1e-4;

impl Node {
    pub fn leaf(pane: PaneId) -> Node {
        Node::Leaf { pane }
    }

    pub fn panes(&self) -> Vec<PaneId> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect(&self, out: &mut Vec<PaneId>) {
        match self {
            Node::Leaf { pane } => out.push(*pane),
            Node::Split { a, b, .. } => {
                a.collect(out);
                b.collect(out);
            }
        }
    }

    pub fn contains(&self, id: PaneId) -> bool {
        match self {
            Node::Leaf { pane } => *pane == id,
            Node::Split { a, b, .. } => a.contains(id) || b.contains(id),
        }
    }

    pub fn split(&mut self, target: PaneId, dir: Dir, new: PaneId) -> bool {
        match self {
            Node::Leaf { pane } if *pane == target => {
                *self = Node::Split {
                    dir,
                    ratio: 0.5,
                    a: Box::new(Node::leaf(target)),
                    b: Box::new(Node::leaf(new)),
                };
                true
            }
            Node::Leaf { .. } => false,
            Node::Split { a, b, .. } => a.split(target, dir, new) || b.split(target, dir, new),
        }
    }

    pub fn remove(self, id: PaneId) -> Option<Node> {
        match self {
            Node::Leaf { pane } if pane == id => None,
            leaf @ Node::Leaf { .. } => Some(leaf),
            Node::Split { dir, ratio, a, b } => match (a.remove(id), b.remove(id)) {
                (Some(a), Some(b)) => Some(Node::Split { dir, ratio, a: Box::new(a), b: Box::new(b) }),
                (Some(n), None) | (None, Some(n)) => Some(n),
                (None, None) => None,
            },
        }
    }

    pub fn swap(&mut self, x: PaneId, y: PaneId) -> bool {
        if x == y || !self.contains(x) || !self.contains(y) {
            return false;
        }
        self.map_leaves(&|p| {
            if p == x {
                y
            } else if p == y {
                x
            } else {
                p
            }
        });
        true
    }

    fn map_leaves(&mut self, f: &dyn Fn(PaneId) -> PaneId) {
        match self {
            Node::Leaf { pane } => *pane = f(*pane),
            Node::Split { a, b, .. } => {
                a.map_leaves(f);
                b.map_leaves(f);
            }
        }
    }

    pub fn rects(&self) -> Vec<(PaneId, Rect)> {
        let mut out = Vec::new();
        self.place(Rect::UNIT, &mut out);
        out
    }

    fn place(&self, area: Rect, out: &mut Vec<(PaneId, Rect)>) {
        match self {
            Node::Leaf { pane } => out.push((*pane, area)),
            Node::Split { dir, ratio, a, b } => {
                let (ra, rb) = split_rect(area, *dir, *ratio);
                a.place(ra, out);
                b.place(rb, out);
            }
        }
    }

    pub fn neighbor(&self, from: PaneId, side: Side) -> Option<PaneId> {
        let rects = self.rects();
        let me = rects.iter().find(|(p, _)| *p == from)?.1;
        let mut best: Option<(PaneId, f32)> = None;
        for (p, r) in &rects {
            if *p == from {
                continue;
            }
            let touches = match side {
                Side::Left => (r.x + r.w - me.x).abs() < EPS,
                Side::Right => (me.x + me.w - r.x).abs() < EPS,
                Side::Up => (r.y + r.h - me.y).abs() < EPS,
                Side::Down => (me.y + me.h - r.y).abs() < EPS,
            };
            if !touches {
                continue;
            }
            let shared = match side {
                Side::Left | Side::Right => overlap(me.y, me.h, r.y, r.h),
                Side::Up | Side::Down => overlap(me.x, me.w, r.x, r.w),
            };
            let better = match best {
                None => true,
                Some((_, o)) => shared > o + EPS,
            };
            if shared > EPS && better {
                best = Some((*p, shared));
            }
        }
        best.map(|(p, _)| p)
    }

    pub fn set_ratio(&mut self, path: &[u8], value: f32) -> bool {
        match (self, path.split_first()) {
            (Node::Split { ratio, .. }, None) => {
                if !value.is_finite() {
                    return false;
                }
                *ratio = value.clamp(MIN_RATIO, MAX_RATIO);
                true
            }
            (Node::Split { a, .. }, Some((0, rest))) => a.set_ratio(rest, value),
            (Node::Split { b, .. }, Some((1, rest))) => b.set_ratio(rest, value),
            _ => false,
        }
    }

    pub fn resize(&mut self, id: PaneId, side: Side, delta: f32) -> bool {
        let Node::Split { dir, ratio, a, b } = self else { return false };
        if a.contains(id) {
            if a.resize(id, side, delta) {
                return true;
            }
            if matches!((*dir, side), (Dir::Right, Side::Right) | (Dir::Down, Side::Down)) {
                *ratio = (*ratio + delta).clamp(MIN_RATIO, MAX_RATIO);
                return true;
            }
        } else if b.contains(id) {
            if b.resize(id, side, delta) {
                return true;
            }
            if matches!((*dir, side), (Dir::Right, Side::Left) | (Dir::Down, Side::Up)) {
                *ratio = (*ratio - delta).clamp(MIN_RATIO, MAX_RATIO);
                return true;
            }
        }
        false
    }

    pub fn is_valid(&self) -> bool {
        let mut seen = std::collections::HashSet::new();
        self.panes().iter().all(|p| seen.insert(*p)) && self.ratios_ok()
    }

    fn ratios_ok(&self) -> bool {
        match self {
            Node::Leaf { .. } => true,
            Node::Split { ratio, a, b, .. } => {
                ratio.is_finite() && (MIN_RATIO..=MAX_RATIO).contains(ratio) && a.ratios_ok() && b.ratios_ok()
            }
        }
    }
}

fn split_rect(area: Rect, dir: Dir, ratio: f32) -> (Rect, Rect) {
    match dir {
        Dir::Right => {
            let w = area.w * ratio;
            (Rect { w, ..area }, Rect { x: area.x + w, w: area.w - w, ..area })
        }
        Dir::Down => {
            let h = area.h * ratio;
            (Rect { h, ..area }, Rect { y: area.y + h, h: area.h - h, ..area })
        }
    }
}

fn overlap(a0: f32, alen: f32, b0: f32, blen: f32) -> f32 {
    (a0 + alen).min(b0 + blen) - a0.max(b0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(n: u32) -> PaneId {
        PaneId(n)
    }

    fn three() -> Node {
        let mut t = Node::leaf(p(1));
        assert!(t.split(p(1), Dir::Right, p(2)));
        assert!(t.split(p(2), Dir::Down, p(3)));
        t
    }

    #[test]
    fn split_and_order() {
        assert_eq!(three().panes(), vec![p(1), p(2), p(3)]);
        let mut t = three();
        assert!(!t.split(p(9), Dir::Right, p(4)));
    }

    #[test]
    fn remove_collapses_to_sibling() {
        let t = three().remove(p(2)).unwrap();
        assert_eq!(t.panes(), vec![p(1), p(3)]);
        let t = t.remove(p(1)).unwrap();
        assert_eq!(t, Node::leaf(p(3)));
        assert_eq!(t.remove(p(3)), None);
    }

    #[test]
    fn neighbors_follow_geometry() {
        let t = three();
        assert_eq!(t.neighbor(p(1), Side::Right), Some(p(2)));
        assert_eq!(t.neighbor(p(2), Side::Down), Some(p(3)));
        assert_eq!(t.neighbor(p(3), Side::Left), Some(p(1)));
        assert_eq!(t.neighbor(p(3), Side::Up), Some(p(2)));
        assert_eq!(t.neighbor(p(1), Side::Left), None);
        assert_eq!(t.neighbor(p(1), Side::Up), None);
    }

    #[test]
    fn rects_cover_the_unit_square() {
        let area: f32 = three().rects().iter().map(|(_, r)| r.w * r.h).sum();
        assert!((area - 1.0).abs() < 1e-5);
    }

    #[test]
    fn swap_exchanges_positions() {
        let mut t = three();
        assert!(t.swap(p(1), p(3)));
        assert_eq!(t.panes(), vec![p(3), p(2), p(1)]);
        assert!(!t.swap(p(1), p(9)));
    }

    #[test]
    fn ratios_are_clamped() {
        let mut t = three();
        assert!(t.set_ratio(&[], 0.99));
        assert!(t.set_ratio(&[1], 0.0));
        assert!(!t.set_ratio(&[0], 0.5), "path ends on a leaf");
        assert!(!t.set_ratio(&[], f32::NAN));
        assert!(t.is_valid());
        let Node::Split { ratio, .. } = &t else { panic!() };
        assert_eq!(*ratio, MAX_RATIO);
    }

    #[test]
    fn resize_moves_the_nearest_matching_border() {
        let mut t = three();
        assert!(t.resize(p(3), Side::Up, 0.1));
        let Node::Split { b, ratio, .. } = &t else { panic!() };
        assert_eq!(*ratio, 0.5);
        let Node::Split { ratio: inner, .. } = b.as_ref() else { panic!() };
        assert!((*inner - 0.4).abs() < 1e-5);
        assert!(t.resize(p(3), Side::Left, 0.1));
        let Node::Split { ratio, .. } = &t else { panic!() };
        assert!((*ratio - 0.4).abs() < 1e-5);
        assert!(!t.resize(p(3), Side::Right, 0.1));
    }

    #[test]
    fn duplicate_panes_are_invalid() {
        let t = Node::Split {
            dir: Dir::Right,
            ratio: 0.5,
            a: Box::new(Node::leaf(p(1))),
            b: Box::new(Node::leaf(p(1))),
        };
        assert!(!t.is_valid());
    }
}
