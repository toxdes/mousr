use std::fmt;

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn center(self) -> (u32, u32) {
        (self.x + self.width / 2, self.y + self.height / 2)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    pub output: String,
    pub bounds: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    pub output: String,
    pub bounds: Rect,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub tiles: Vec<Tile>,
    pub label_length: u8,
}

impl Layout {
    pub fn exact(&self, label: &str) -> Option<usize> {
        self.tiles.iter().position(|tile| tile.label == label)
    }

    pub fn matching(&self, prefix: &str) -> impl Iterator<Item = (usize, &Tile)> {
        self.tiles
            .iter()
            .enumerate()
            .filter(move |(_, tile)| tile.label.starts_with(prefix))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Settings {
    pub min_tile_width: u32,
    pub min_tile_height: u32,
    pub max_label_length: u8,
    pub max_cells: usize,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GridError {
    #[error("grid needs at least one region")]
    NoRegions,
    #[error("region {0} is smaller than the configured minimum tile size")]
    RegionTooSmall(String),
    #[error("{outputs} outputs cannot fit within a capacity of {capacity} cells")]
    CapacityTooSmall { outputs: usize, capacity: usize },
    #[error("grid dimensions overflow")]
    Overflow,
}

pub fn build(regions: &[Region], settings: Settings) -> Result<Layout, GridError> {
    if regions.is_empty() {
        return Err(GridError::NoRegions);
    }
    if let Some(region) = regions.iter().find(|region| {
        region.bounds.width < settings.min_tile_width
            || region.bounds.height < settings.min_tile_height
    }) {
        return Err(GridError::RegionTooSmall(region.output.clone()));
    }

    let label_capacity = checked_pow(26, settings.max_label_length)?;
    let capacity = settings.max_cells.min(label_capacity);
    if capacity < regions.len() {
        return Err(GridError::CapacityTooSmall {
            outputs: regions.len(),
            capacity,
        });
    }

    let scale = coarsening_scale(regions, settings, capacity)?;
    let dimensions: Vec<(u32, u32)> = regions
        .iter()
        .map(|region| dimensions(region.bounds, settings, scale))
        .collect();
    let tile_count = dimensions
        .iter()
        .try_fold(0_usize, |total, (columns, rows)| {
            let count = usize::try_from(columns.checked_mul(*rows).ok_or(GridError::Overflow)?)
                .map_err(|_| GridError::Overflow)?;
            total.checked_add(count).ok_or(GridError::Overflow)
        })?;
    let label_length = label_length(tile_count, settings.max_label_length)?;
    let mut tiles = Vec::with_capacity(tile_count);

    for (region, (columns, rows)) in regions.iter().zip(dimensions) {
        for row in 0..rows {
            for column in 0..columns {
                let x0 = partition(region.bounds.x, region.bounds.width, column, columns)?;
                let x1 = partition(region.bounds.x, region.bounds.width, column + 1, columns)?;
                let y0 = partition(region.bounds.y, region.bounds.height, row, rows)?;
                let y1 = partition(region.bounds.y, region.bounds.height, row + 1, rows)?;
                tiles.push(Tile {
                    output: region.output.clone(),
                    bounds: Rect {
                        x: x0,
                        y: y0,
                        width: x1 - x0,
                        height: y1 - y0,
                    },
                    label: encode_label(tiles.len(), label_length),
                });
            }
        }
    }

    Ok(Layout {
        tiles,
        label_length,
    })
}

pub fn descend(tile: &Tile, settings: Settings) -> Result<Option<Layout>, GridError> {
    let region = Region {
        output: tile.output.clone(),
        bounds: tile.bounds,
    };
    match build(&[region], settings) {
        Ok(layout) if layout.tiles.len() >= 2 => Ok(Some(layout)),
        Ok(_) | Err(GridError::RegionTooSmall(_)) => Ok(None),
        Err(error) => Err(error),
    }
}

fn coarsening_scale(
    regions: &[Region],
    settings: Settings,
    capacity: usize,
) -> Result<f64, GridError> {
    if cell_count(regions, settings, 1.0)? <= capacity {
        return Ok(1.0);
    }

    let mut low = 1.0;
    let mut high = 2.0;
    while cell_count(regions, settings, high)? > capacity {
        low = high;
        high *= 2.0;
        if !high.is_finite() {
            return Err(GridError::Overflow);
        }
    }
    for _ in 0..64 {
        let middle = (low + high) / 2.0;
        if cell_count(regions, settings, middle)? > capacity {
            low = middle;
        } else {
            high = middle;
        }
    }
    Ok(high)
}

fn cell_count(regions: &[Region], settings: Settings, scale: f64) -> Result<usize, GridError> {
    regions.iter().try_fold(0_usize, |total, region| {
        let (columns, rows) = dimensions(region.bounds, settings, scale);
        let count = usize::try_from(columns.checked_mul(rows).ok_or(GridError::Overflow)?)
            .map_err(|_| GridError::Overflow)?;
        total.checked_add(count).ok_or(GridError::Overflow)
    })
}

fn dimensions(bounds: Rect, settings: Settings, scale: f64) -> (u32, u32) {
    let tile_width = f64::from(settings.min_tile_width) * scale;
    let tile_height = f64::from(settings.min_tile_height) * scale;
    let columns = (f64::from(bounds.width) / tile_width).floor().max(1.0) as u32;
    let rows = (f64::from(bounds.height) / tile_height).floor().max(1.0) as u32;
    (columns, rows)
}

fn partition(origin: u32, length: u32, index: u32, count: u32) -> Result<u32, GridError> {
    let offset = u64::from(length)
        .checked_mul(u64::from(index))
        .ok_or(GridError::Overflow)?
        / u64::from(count);
    let coordinate = u64::from(origin)
        .checked_add(offset)
        .ok_or(GridError::Overflow)?;
    u32::try_from(coordinate).map_err(|_| GridError::Overflow)
}

fn label_length(count: usize, maximum: u8) -> Result<u8, GridError> {
    for length in 1..=maximum {
        if checked_pow(26, length)? >= count {
            return Ok(length);
        }
    }
    Err(GridError::Overflow)
}

fn checked_pow(base: usize, exponent: u8) -> Result<usize, GridError> {
    (0..exponent).try_fold(1_usize, |value, _| {
        value.checked_mul(base).ok_or(GridError::Overflow)
    })
}

fn encode_label(mut index: usize, length: u8) -> String {
    let mut bytes = vec![b'a'; usize::from(length)];
    for byte in bytes.iter_mut().rev() {
        *byte += (index % 26) as u8;
        index /= 26;
    }
    bytes.into_iter().map(char::from).collect()
}

impl fmt::Display for Rect {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}x{}+{}+{}",
            self.width, self.height, self.x, self.y
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(labels: u8, max_cells: usize) -> Settings {
        Settings {
            min_tile_width: 24,
            min_tile_height: 24,
            max_label_length: labels,
            max_cells,
        }
    }

    fn region(name: &str, width: u32, height: u32) -> Region {
        Region {
            output: name.to_owned(),
            bounds: Rect {
                x: 0,
                y: 0,
                width,
                height,
            },
        }
    }

    #[test]
    fn derives_fixed_length_labels() {
        let layout = build(&[region("DP-1", 1920, 1080)], settings(2, 4096)).unwrap();
        assert_eq!(layout.label_length, 2);
        assert_eq!(layout.tiles[0].label, "aa");
        assert!(layout.tiles.len() <= 676);
        assert!(layout.tiles.iter().all(|tile| tile.label.len() == 2));
    }

    #[test]
    fn exactly_covers_a_region() {
        let layout = build(&[region("DP-1", 1919, 1079)], settings(1, 26)).unwrap();
        let right = layout
            .tiles
            .iter()
            .map(|tile| tile.bounds.x + tile.bounds.width)
            .max();
        let bottom = layout
            .tiles
            .iter()
            .map(|tile| tile.bounds.y + tile.bounds.height)
            .max();
        assert_eq!(right, Some(1919));
        assert_eq!(bottom, Some(1079));
        assert!(layout.tiles.iter().all(|tile| tile.bounds.width >= 24));
        assert!(layout.tiles.iter().all(|tile| tile.bounds.height >= 24));
    }

    #[test]
    fn gives_multiple_outputs_unique_labels() {
        let layout = build(
            &[region("DP-1", 1920, 1080), region("HDMI-A-1", 1280, 1024)],
            settings(2, 400),
        )
        .unwrap();
        let unique: std::collections::HashSet<_> =
            layout.tiles.iter().map(|tile| &tile.label).collect();
        assert_eq!(unique.len(), layout.tiles.len());
        assert!(layout.tiles.iter().any(|tile| tile.output == "DP-1"));
        assert!(layout.tiles.iter().any(|tile| tile.output == "HDMI-A-1"));
    }

    #[test]
    fn descent_stops_at_minimum_size() {
        let tile = Tile {
            output: "DP-1".into(),
            bounds: Rect {
                x: 0,
                y: 0,
                width: 30,
                height: 30,
            },
            label: "a".into(),
        };
        assert_eq!(descend(&tile, settings(2, 4096)).unwrap(), None);
    }
}
