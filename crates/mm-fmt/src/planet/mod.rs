//! `core.fmt` 里星球相关格式规则的等价移植。
//!
//! 第一批：[`mesh_types`]（`MeshTypes.java`）、[`grids`]（`PlanetGrids.java`）、
//! [`orbits`]（`PlanetOrbits.java`）。
//! 后续批次（`SimplexNoise`/`PlanetMeshes`/`PlanetTemplates`）在本目录继续追加模块。

pub mod grids;
pub mod mesh_types;
pub mod meshes;
pub mod noise;
pub mod orbits;
pub mod templates;

pub use grids::{Corner, CornerRef, Grid, Tile, TileRef, Vec3};
pub use mesh_types::{Field, Spec};
pub use meshes::{build_model, Layer, MeshModel, Mesher, Rgb};
pub use noise::{noise3d, raw3d};
pub use orbits::Body;
pub use templates::template_for;
