//! Where each widget sits on the desktop grid, how big it is and whether it's collapsed. Changed
//! on the desktop by dragging, the resize handle and the right-click menu; kept in
//! `~/.local/state/hyprwidgets/layout.json`. config.toml only gives the starting corner.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Grid cell, the gap between cells, and the two added: a Mini widget is one cell, a Small one
/// 2x2 cells plus the gap (170 px), Medium 4x2, Large 4x4.
pub const CELL: i32 = 77;
pub const GAP: i32 = 16;
pub const PITCH: i32 = CELL + GAP;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Size {
    Mini,
    Small,
    Medium,
    Large,
}

impl Size {
    /// Columns and rows on the grid.
    pub fn cells(self) -> (i32, i32) {
        match self {
            Size::Mini => (1, 1),
            Size::Small => (2, 2),
            Size::Medium => (4, 2),
            Size::Large => (4, 4),
        }
    }

    pub fn px(self) -> (i32, i32) {
        let (w, h) = self.cells();
        (w * PITCH - GAP, h * PITCH - GAP)
    }

    pub fn name(self) -> &'static str {
        match self {
            Size::Mini => "mini",
            Size::Small => "small",
            Size::Medium => "medium",
            Size::Large => "large",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Size::Mini => "Mini",
            Size::Small => "Small",
            Size::Medium => "Medium",
            Size::Large => "Large",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Item {
    pub col: i32,
    pub row: i32,
    pub size: Size,
    #[serde(default)]
    pub collapsed: bool,
}

impl Item {
    /// The cells it covers: a collapsed card is one row high.
    pub fn span(&self) -> (i32, i32) {
        let (w, h) = self.size.cells();
        (w, if self.collapsed { 1 } else { h })
    }

    fn overlaps(&self, o: &Item) -> bool {
        let ((w, h), (ow, oh)) = (self.span(), o.span());
        self.col < o.col + ow && o.col < self.col + w && self.row < o.row + oh && o.row < self.row + h
    }
}

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub items: BTreeMap<String, Item>,
}

impl Layout {
    pub fn load() -> Layout {
        std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let p = path();
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(p, text);
        }
    }
}

/// The desktop as a grid of `cols` x `rows` cells.
#[derive(Clone, Copy)]
pub struct Grid {
    pub cols: i32,
    pub rows: i32,
}

impl Grid {
    /// How many cells fit in `w` x `h` pixels.
    pub fn fitting(w: i32, h: i32) -> Grid {
        Grid { cols: ((w + GAP) / PITCH).max(4), rows: ((h + GAP) / PITCH).max(4) }
    }

    fn inside(&self, it: &Item) -> bool {
        let (w, h) = it.span();
        it.col >= 0 && it.row >= 0 && it.col + w <= self.cols && it.row + h <= self.rows
    }

    pub fn free(&self, it: &Item, others: &[Item]) -> bool {
        self.inside(it) && others.iter().all(|o| !it.overlaps(o))
    }

    /// The free spot closest to where `it` wants to go, if there is any room at all.
    pub fn nearest_free(&self, it: Item, others: &[Item]) -> Option<Item> {
        let mut best: Option<(i32, Item)> = None;
        for row in 0..self.rows {
            for col in 0..self.cols {
                let c = Item { col, row, ..it };
                if self.free(&c, others) {
                    let d = (col - it.col).pow(2) + (row - it.row).pow(2);
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, c));
                    }
                }
            }
        }
        best.map(|(_, c)| c)
    }

    /// Where a new widget goes: the first free spot in 4-column bands, starting from the
    /// configured corner (`top-left`, `bottom-right`, ...).
    pub fn auto_place(&self, size: Size, corner: &str, others: &[Item]) -> Item {
        let from_right = corner.ends_with("right");
        let from_bottom = corner.starts_with("bottom");
        let (w, h) = size.cells();
        let bands = (self.cols / 4).max(1);
        for b in 0..bands {
            let (lo, hi) = if from_right { (self.cols - (b + 1) * 4, self.cols - b * 4) } else { (b * 4, b * 4 + 4) };
            for r in 0..self.rows {
                let row = if from_bottom { self.rows - h - r } else { r };
                let cols: Vec<i32> = if from_right { (lo..=hi - w).rev().collect() } else { (lo..=hi - w).collect() };
                for col in cols {
                    let it = Item { col, row, size, collapsed: false };
                    if self.free(&it, others) {
                        return it;
                    }
                }
            }
        }
        let it = Item { col: 0, row: 0, size, collapsed: false };
        self.nearest_free(it, others).unwrap_or(it)
    }
}

pub fn path() -> PathBuf {
    dirs::state_dir().unwrap_or_else(std::env::temp_dir).join("hyprwidgets").join("layout.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_place_fills_a_band() {
        let g = Grid { cols: 20, rows: 10 };
        let a = g.auto_place(Size::Small, "top-left", &[]);
        let b = g.auto_place(Size::Small, "top-left", &[a]);
        let c = g.auto_place(Size::Large, "top-left", &[a, b]);
        assert_eq!((a.col, a.row, b.col, b.row, c.col, c.row), (0, 0, 2, 0, 0, 2));
        let r = g.auto_place(Size::Medium, "top-right", &[]);
        assert_eq!((r.col, r.row), (16, 0));
    }

    #[test]
    fn collapsed_items_free_their_rows() {
        let g = Grid { cols: 8, rows: 8 };
        let big = Item { col: 0, row: 0, size: Size::Large, collapsed: true };
        let below = Item { col: 0, row: 1, size: Size::Small, collapsed: false };
        assert!(g.free(&below, &[big]));
        let open = Item { collapsed: false, ..big };
        assert!(!g.free(&below, &[open]));
        let moved = g.nearest_free(below, &[open]).unwrap();
        assert!(g.free(&moved, &[open]));
    }
}
