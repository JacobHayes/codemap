//! The query layer: one `Query` in, one `Answer` out. Every surface (CLI, MCP, web UI) is a thin
//! shell over it.

mod config;
mod detect;
mod facts;
mod index;
mod measure;
mod query;
mod reader;
mod role;
mod store;
mod syntax;
mod text;
mod worktree;

pub use config::{Acknowledgement, Config, ConfigError, Granularity, Rule};
pub use detect::{Area, DETECTORS, Detector, Evidence, Finding, SubjectKind};
pub use facts::{FileFacts, LineFact, RefFact, SymbolFact, SymbolKind, extract};
pub use index::{IndexError, IndexReport, index};
pub use measure::{
    Base, Filter, Measure, MeasureDef, MeasureRef, Term, catalog as measure_catalog,
};
pub use query::{
    Answer, AreaCounts, AreaDetail, AreaList, Catalog, DEPTH_DEFAULT, DetectorCount, DetectorInfo,
    Dimension, Envelope, Hierarchy, Overview, Query, SCALE_STEPS, SCHEMA, Scale, ScaleKind, Share,
    SizeInfo, Spacing, Totals, Tree, TreeNode, answer,
};
pub use reader::{Index, QueryError};
pub use role::role_for_path;
pub use store::STATE_DIR;
