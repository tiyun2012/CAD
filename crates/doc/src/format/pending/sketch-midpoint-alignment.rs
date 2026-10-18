//! Sketch Horizontal/Vertical constraints can save derived line midpoint references in an
//! `alignment` constraint without adding helper points. Older designs keep their existing
//! point/line constraints unchanged; no rewrite is needed. Older releases refuse the newer
//! format number rather than dropping this relation.
