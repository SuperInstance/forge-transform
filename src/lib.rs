use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Specification for a transform, used for serialization/deserialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformSpec {
    pub name: String,
    pub params: HashMap<String, String>,
}

/// A tile of data that can be transformed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileData {
    pub id: Uuid,
    pub kind: String,
    pub payload: Vec<u8>,
    pub index: u64,
    pub meta: HashMap<String, String>,
}

impl TileData {
    pub fn new(kind: impl Into<String>, payload: Vec<u8>, index: u64) -> Self {
        Self {
            id: Uuid::new_v4(),
            kind: kind.into(),
            payload,
            index,
            meta: HashMap::new(),
        }
    }

    pub fn with_meta(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.meta.insert(key.into(), value.into());
        self
    }
}

/// Core trait for tile transforms.
pub trait TileTransform: Send + Sync {
    fn name(&self) -> &str;
    fn transform(&self, tiles: Vec<TileData>) -> Result<Vec<TileData>, TransformError>;
    fn conservation_ratio(&self, input: &[TileData], output: &[TileData]) -> f64 {
        if input.is_empty() {
            1.0
        } else {
            output.len() as f64 / input.len() as f64
        }
    }
}

#[derive(Debug)]
pub enum TransformError {
    MissingParam(String),
    InvalidParam(String),
    Pipeline { stage: usize, message: String },
}

impl std::fmt::Display for TransformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransformError::MissingParam(p) => write!(f, "missing required parameter: {p}"),
            TransformError::InvalidParam(p) => write!(f, "invalid parameter value: {p}"),
            TransformError::Pipeline { stage, message } => write!(f, "pipeline error at stage {stage}: {message}"),
        }
    }
}

impl std::error::Error for TransformError {}

// ── Built-in transforms ──────────────────────────────────────────────

/// Filter tiles by a meta field comparison.
pub struct FilterTransform {
    field: String,
    op: FilterOp,
    value: String,
}

enum FilterOp {
    Eq,
    Ne,
    Gt,
    Lt,
    Contains,
}

impl FilterTransform {
    pub fn new(field: impl Into<String>, op: &str, value: impl Into<String>) -> Result<Self, TransformError> {
        let op = match op {
            "eq" => FilterOp::Eq,
            "ne" => FilterOp::Ne,
            "gt" => FilterOp::Gt,
            "lt" => FilterOp::Lt,
            "contains" => FilterOp::Contains,
            other => return Err(TransformError::InvalidParam(format!("unknown op: {other}"))),
        };
        Ok(Self { field: field.into(), op, value: value.into() })
    }

    pub fn from_params(params: &HashMap<String, String>) -> Result<Self, TransformError> {
        let field = params.get("field").ok_or_else(|| TransformError::MissingParam("field".into()))?.clone();
        let op = params.get("op").ok_or_else(|| TransformError::MissingParam("op".into()))?;
        let value = params.get("value").ok_or_else(|| TransformError::MissingParam("value".into()))?.clone();
        Self::new(&field, op, &value)
    }

    fn matches(&self, tile: &TileData) -> bool {
        let actual = match tile.meta.get(&self.field) {
            Some(v) => v,
            None => return false,
        };
        match self.op {
            FilterOp::Eq => actual == &self.value,
            FilterOp::Ne => actual != &self.value,
            FilterOp::Gt => actual > &self.value,
            FilterOp::Lt => actual < &self.value,
            FilterOp::Contains => actual.contains(&self.value),
        }
    }
}

impl TileTransform for FilterTransform {
    fn name(&self) -> &str { "filter" }
    fn transform(&self, tiles: Vec<TileData>) -> Result<Vec<TileData>, TransformError> {
        Ok(tiles.into_iter().filter(|t| self.matches(t)).collect())
    }
}

/// Sort tiles by a meta field.
pub struct SortTransform {
    field: String,
    descending: bool,
}

impl SortTransform {
    pub fn new(field: impl Into<String>, descending: bool) -> Self {
        Self { field: field.into(), descending }
    }

    pub fn from_params(params: &HashMap<String, String>) -> Result<Self, TransformError> {
        let field = params.get("field").ok_or_else(|| TransformError::MissingParam("field".into()))?.clone();
        let order = params.get("order").map(|s| s.as_str()).unwrap_or("asc");
        Ok(Self::new(&field, order == "desc"))
    }
}

impl TileTransform for SortTransform {
    fn name(&self) -> &str { "sort" }
    fn transform(&self, mut tiles: Vec<TileData>) -> Result<Vec<TileData>, TransformError> {
        let field = self.field.clone();
        tiles.sort_by(|a, b| {
            let av = a.meta.get(&field).map(|s| s.as_str()).unwrap_or("");
            let bv = b.meta.get(&field).map(|s| s.as_str()).unwrap_or("");
            if self.descending { bv.cmp(av) } else { av.cmp(bv) }
        });
        Ok(tiles)
    }
}

/// Apply a byte-level operation to each tile's payload.
pub struct MapTransform {
    op: MapOp,
}

enum MapOp {
    Uppercase,
    Lowercase,
    Trim,
    Reverse,
    Base64Encode,
    Base64Decode,
}

impl MapTransform {
    pub fn from_params(params: &HashMap<String, String>) -> Result<Self, TransformError> {
        let op = params.get("op").ok_or_else(|| TransformError::MissingParam("op".into()))?;
        let op = match op.as_str() {
            "uppercase" => MapOp::Uppercase,
            "lowercase" => MapOp::Lowercase,
            "trim" => MapOp::Trim,
            "reverse" => MapOp::Reverse,
            "base64_encode" => MapOp::Base64Encode,
            "base64_decode" => MapOp::Base64Decode,
            other => return Err(TransformError::InvalidParam(format!("unknown map op: {other}"))),
        };
        Ok(Self { op })
    }

    fn apply(&self, payload: &[u8]) -> Vec<u8> {
        match self.op {
            MapOp::Uppercase => payload.iter().map(|b| b.to_ascii_uppercase()).collect(),
            MapOp::Lowercase => payload.iter().map(|b| b.to_ascii_lowercase()).collect(),
            MapOp::Trim => {
                let s = String::from_utf8_lossy(payload);
                s.trim().as_bytes().to_vec()
            }
            MapOp::Reverse => payload.iter().rev().copied().collect(),
            MapOp::Base64Encode => {
                base64_light::base64_encode(payload).into_bytes()
            }
            MapOp::Base64Decode => {
                base64_light::base64_decode_bytes(payload).unwrap_or_default()
            }
        }
    }
}

impl TileTransform for MapTransform {
    fn name(&self) -> &str { "map" }
    fn transform(&self, tiles: Vec<TileData>) -> Result<Vec<TileData>, TransformError> {
        Ok(tiles.into_iter().map(|mut t| { t.payload = self.apply(&t.payload); t }).collect())
    }
}

/// Group tiles into chunks of N.
pub struct ChunkTransform {
    size: usize,
}

impl ChunkTransform {
    pub fn new(size: usize) -> Self { Self { size: std::cmp::max(size, 1) } }

    pub fn from_params(params: &HashMap<String, String>) -> Result<Self, TransformError> {
        let size: usize = params.get("size").ok_or_else(|| TransformError::MissingParam("size".into()))?
            .parse().map_err(|_| TransformError::InvalidParam("size must be a positive integer".into()))?;
        Ok(Self::new(size))
    }
}

impl TileTransform for ChunkTransform {
    fn name(&self) -> &str { "chunk" }
    fn transform(&self, tiles: Vec<TileData>) -> Result<Vec<TileData>, TransformError> {
        let mut result = Vec::new();
        for chunk in tiles.chunks(self.size) {
            let mut merged = TileData::new("chunk", Vec::new(), result.len() as u64);
            for t in chunk {
                merged.payload.extend_from_slice(&t.payload);
            }
            result.push(merged);
        }
        Ok(result)
    }
}

/// Merge tiles by concatenating payloads.
pub struct FlattenTransform {
    separator: Vec<u8>,
}

impl FlattenTransform {
    pub fn new(separator: impl Into<Vec<u8>>) -> Self { Self { separator: separator.into() } }

    pub fn from_params(params: &HashMap<String, String>) -> Result<Self, TransformError> {
        let sep = params.get("separator").map(|s| s.as_bytes().to_vec()).unwrap_or_default();
        Ok(Self::new(sep))
    }
}

impl TileTransform for FlattenTransform {
    fn name(&self) -> &str { "flatten" }
    fn transform(&self, tiles: Vec<TileData>) -> Result<Vec<TileData>, TransformError> {
        if tiles.is_empty() {
            return Ok(vec![]);
        }
        let mut payload = Vec::new();
        for (i, t) in tiles.iter().enumerate() {
            if i > 0 {
                payload.extend_from_slice(&self.separator);
            }
            payload.extend_from_slice(&t.payload);
        }
        Ok(vec![TileData {
            id: Uuid::new_v4(),
            kind: "flattened".into(),
            payload,
            index: 0,
            meta: HashMap::new(),
        }])
    }
}

/// Remove duplicate tiles by payload hash.
pub struct DeduplicateTransform;

impl TileTransform for DeduplicateTransform {
    fn name(&self) -> &str { "deduplicate" }
    fn transform(&self, tiles: Vec<TileData>) -> Result<Vec<TileData>, TransformError> {
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        Ok(tiles.into_iter().filter(|t| seen.insert(t.payload.clone())).collect())
    }
}

/// Take the first N tiles.
pub struct HeadTransform {
    count: usize,
}

impl HeadTransform {
    pub fn new(count: usize) -> Self { Self { count } }

    pub fn from_params(params: &HashMap<String, String>) -> Result<Self, TransformError> {
        let count: usize = params.get("count").ok_or_else(|| TransformError::MissingParam("count".into()))?
            .parse().map_err(|_| TransformError::InvalidParam("count must be a positive integer".into()))?;
        Ok(Self::new(count))
    }
}

impl TileTransform for HeadTransform {
    fn name(&self) -> &str { "head" }
    fn transform(&self, tiles: Vec<TileData>) -> Result<Vec<TileData>, TransformError> {
        Ok(tiles.into_iter().take(self.count).collect())
    }
}

/// Take the last N tiles.
pub struct TailTransform {
    count: usize,
}

impl TailTransform {
    pub fn new(count: usize) -> Self { Self { count } }

    pub fn from_params(params: &HashMap<String, String>) -> Result<Self, TransformError> {
        let count: usize = params.get("count").ok_or_else(|| TransformError::MissingParam("count".into()))?
            .parse().map_err(|_| TransformError::InvalidParam("count must be a positive integer".into()))?;
        Ok(Self::new(count))
    }
}

impl TileTransform for TailTransform {
    fn name(&self) -> &str { "tail" }
    fn transform(&self, tiles: Vec<TileData>) -> Result<Vec<TileData>, TransformError> {
        let skip = tiles.len().saturating_sub(self.count);
        Ok(tiles.into_iter().skip(skip).collect())
    }
}

/// Take every Nth tile.
pub struct SampleTransform {
    rate: usize,
}

impl SampleTransform {
    pub fn new(rate: usize) -> Self { Self { rate: std::cmp::max(rate, 1) } }

    pub fn from_params(params: &HashMap<String, String>) -> Result<Self, TransformError> {
        let rate: usize = params.get("rate").ok_or_else(|| TransformError::MissingParam("rate".into()))?
            .parse().map_err(|_| TransformError::InvalidParam("rate must be a positive integer".into()))?;
        Ok(Self::new(rate))
    }
}

impl TileTransform for SampleTransform {
    fn name(&self) -> &str { "sample" }
    fn transform(&self, tiles: Vec<TileData>) -> Result<Vec<TileData>, TransformError> {
        Ok(tiles.into_iter().step_by(self.rate).collect())
    }
}

/// Chain multiple transforms into a pipeline.
pub struct PipelineTransform {
    transforms: Vec<Box<dyn TileTransform>>,
}

impl PipelineTransform {
    pub fn new(transforms: Vec<Box<dyn TileTransform>>) -> Self { Self { transforms } }
}

impl TileTransform for PipelineTransform {
    fn name(&self) -> &str { "pipeline" }
    fn transform(&self, tiles: Vec<TileData>) -> Result<Vec<TileData>, TransformError> {
        let mut current = tiles;
        for (i, t) in self.transforms.iter().enumerate() {
            current = t.transform(current).map_err(|e| TransformError::Pipeline {
                stage: i,
                message: e.to_string(),
            })?;
        }
        Ok(current)
    }
}

// ── TransformChain with CR tracking ──────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageResult {
    pub name: String,
    pub input_count: usize,
    pub output_count: usize,
    pub conservation_ratio: f64,
}

pub struct TransformChain {
    transforms: Vec<Box<dyn TileTransform>>,
}

impl TransformChain {
    pub fn new(transforms: Vec<Box<dyn TileTransform>>) -> Self { Self { transforms } }

    pub fn execute(&self, tiles: Vec<TileData>) -> Result<(Vec<TileData>, Vec<StageResult>), TransformError> {
        let mut current = tiles;
        let mut stages = Vec::new();
        for t in &self.transforms {
            let input_count = current.len();
            current = t.transform(current)?;
            let output_count = current.len();
            let cr = if input_count == 0 { 1.0 } else { output_count as f64 / input_count as f64 };
            stages.push(StageResult {
                name: t.name().to_string(),
                input_count,
                output_count,
                conservation_ratio: cr,
            });
        }
        Ok((current, stages))
    }
}

// ── Simple base64 (no external dep) ──────────────────────────────────

mod base64_light {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn base64_encode(data: &[u8]) -> String {
        let mut out = String::new();
        let mut i = 0;
        while i < data.len() {
            let b0 = data[i] as u32;
            let b1 = if i + 1 < data.len() { data[i + 1] as u32 } else { 0 };
            let b2 = if i + 2 < data.len() { data[i + 2] as u32 } else { 0 };
            let triple = (b0 << 16) | (b1 << 8) | b2;
            out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
            out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
            out.push(if i + 1 < data.len() { TABLE[((triple >> 6) & 0x3F) as usize] as char } else { '=' });
            out.push(if i + 2 < data.len() { TABLE[(triple & 0x3F) as usize] as char } else { '=' });
            i += 3;
        }
        out
    }

    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    pub fn base64_decode_bytes(data: &[u8]) -> Option<Vec<u8>> {
        let clean: Vec<u8> = data.iter().copied().filter(|&c| c != b'=' && c != b'\n' && c != b'\r' && c != b' ').collect();
        if !clean.len().is_multiple_of(4) && data.contains(&b'=') {
            // padded but wrong length — still try
        }
        let mut out = Vec::new();
        let mut i = 0;
        while i + 4 <= clean.len() || (i < clean.len() && clean.len() - i >= 2) {
            let v0 = val(clean.get(i).copied()?)?;
            let v1 = val(clean.get(i + 1).copied()?)?;
            let v2 = clean.get(i + 2).and_then(|&c| val(c));
            let v3 = clean.get(i + 3).and_then(|&c| val(c));
            let triple = (v0 << 18) | (v1 << 12) | (v2.unwrap_or(0) << 6) | v3.unwrap_or(0);
            out.push(((triple >> 16) & 0xFF) as u8);
            if v2.is_some() { out.push(((triple >> 8) & 0xFF) as u8); }
            if v3.is_some() { out.push((triple & 0xFF) as u8); }
            i += 4;
        }
        Some(out)
    }

        #[allow(dead_code)]
    pub fn base64_encode_bytes(data: &[u8]) -> Vec<u8> {
        base64_encode(data).into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(payload: &[u8], index: u64) -> TileData {
        TileData::new("test", payload.to_vec(), index)
    }

    fn tile_with_meta(payload: &[u8], index: u64, key: &str, val: &str) -> TileData {
        tile(payload, index).with_meta(key, val)
    }

    // 1. FilterTransform
    #[test]
    fn filter_eq() {
        let tiles = vec![
            tile_with_meta(b"a", 0, "x", "1"),
            tile_with_meta(b"b", 1, "x", "2"),
            tile_with_meta(b"c", 2, "x", "1"),
        ];
        let t = FilterTransform::new("x", "eq", "1").unwrap();
        let out = t.transform(tiles).unwrap();
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn filter_contains() {
        let tiles = vec![
            tile_with_meta(b"a", 0, "name", "hello world"),
            tile_with_meta(b"b", 1, "name", "goodbye"),
        ];
        let t = FilterTransform::new("name", "contains", "hello").unwrap();
        let out = t.transform(tiles).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].payload, b"a");
    }

    // 2. SortTransform
    #[test]
    fn sort_ascending() {
        let tiles = vec![
            tile_with_meta(b"c", 0, "k", "3"),
            tile_with_meta(b"a", 1, "k", "1"),
            tile_with_meta(b"b", 2, "k", "2"),
        ];
        let t = SortTransform::new("k", false);
        let out = t.transform(tiles).unwrap();
        assert_eq!(out[0].payload, b"a");
        assert_eq!(out[2].payload, b"c");
    }

    // 3. MapTransform
    #[test]
    fn map_uppercase() {
        let tiles = vec![tile(b"hello", 0)];
        let mut params = HashMap::new();
        params.insert("op".into(), "uppercase".into());
        let t = MapTransform::from_params(&params).unwrap();
        let out = t.transform(tiles).unwrap();
        assert_eq!(out[0].payload, b"HELLO");
    }

    #[test]
    fn map_reverse() {
        let tiles = vec![tile(b"abcd", 0)];
        let mut params = HashMap::new();
        params.insert("op".into(), "reverse".into());
        let t = MapTransform::from_params(&params).unwrap();
        let out = t.transform(tiles).unwrap();
        assert_eq!(out[0].payload, b"dcba");
    }

    // 4. ChunkTransform
    #[test]
    fn chunk_basic() {
        let tiles: Vec<TileData> = (0..5).map(|i| tile(&[i], i as u64)).collect();
        let t = ChunkTransform::new(2);
        let out = t.transform(tiles).unwrap();
        assert_eq!(out.len(), 3); // [0,1] [2,3] [4]
    }

    // 5. FlattenTransform
    #[test]
    fn flatten_with_separator() {
        let tiles = vec![tile(b"a", 0), tile(b"b", 1), tile(b"c", 2)];
        let t = FlattenTransform::new(b",");
        let out = t.transform(tiles).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].payload, b"a,b,c");
    }

    // 6. DeduplicateTransform
    #[test]
    fn dedup() {
        let tiles = vec![tile(b"a", 0), tile(b"b", 1), tile(b"a", 2)];
        let out = DeduplicateTransform.transform(tiles).unwrap();
        assert_eq!(out.len(), 2);
    }

    // 7. HeadTransform
    #[test]
    fn head() {
        let tiles: Vec<TileData> = (0..10).map(|i| tile(&[i], i as u64)).collect();
        let t = HeadTransform::new(3);
        let out = t.transform(tiles).unwrap();
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].index, 0);
    }

    // 8. TailTransform
    #[test]
    fn tail() {
        let tiles: Vec<TileData> = (0..10).map(|i| tile(&[i], i as u64)).collect();
        let t = TailTransform::new(3);
        let out = t.transform(tiles).unwrap();
        assert_eq!(out.len(), 3);
        assert_eq!(out[2].index, 9);
    }

    // 9. SampleTransform
    #[test]
    fn sample_every_3rd() {
        let tiles: Vec<TileData> = (0..9).map(|i| tile(&[i], i as u64)).collect();
        let t = SampleTransform::new(3);
        let out = t.transform(tiles).unwrap();
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].payload, vec![0u8]);
        assert_eq!(out[1].payload, vec![3u8]);
    }

    // 10. Pipeline
    #[test]
    fn pipeline_filter_then_sort() {
        let tiles = vec![
            tile_with_meta(b"c", 0, "x", "1"),
            tile_with_meta(b"a", 1, "x", "2"),
            tile_with_meta(b"b", 2, "x", "1"),
        ];
        let pipe = PipelineTransform::new(vec![
            Box::new(FilterTransform::new("x", "eq", "1").unwrap()),
            Box::new(SortTransform::new("x", false)), // sort by x asc (both "1" so stable)
        ]);
        let out = pipe.transform(tiles).unwrap();
        assert_eq!(out.len(), 2);
    }

    // 11. TransformChain with CR tracking
    #[test]
    fn chain_tracks_cr() {
        let tiles: Vec<TileData> = (0..10).map(|i| tile(&[i], i as u64)).collect();
        let chain = TransformChain::new(vec![
            Box::new(HeadTransform::new(5)),
            Box::new(SampleTransform::new(2)),
        ]);
        let (out, stages) = chain.execute(tiles).unwrap();
        assert_eq!(out.len(), 3); // 5 kept, then every 2nd: indices 0,2,4
        assert_eq!(stages[0].conservation_ratio, 0.5);
        assert_eq!(stages[1].conservation_ratio, 0.6); // 3/5
    }

    // 12. Conservation ratio on empty
    #[test]
    fn cr_empty_input() {
        let t = HeadTransform::new(5);
        let tiles = vec![];
        assert_eq!(t.conservation_ratio(&tiles, &vec![]), 1.0);
    }

    // 13. Empty input through filter
    #[test]
    fn filter_empty() {
        let t = FilterTransform::new("x", "eq", "1").unwrap();
        let out = t.transform(vec![]).unwrap();
        assert!(out.is_empty());
    }

    // 14. Single tile
    #[test]
    fn single_tile_pipeline() {
        let tiles = vec![tile(b"hello", 0)];
        let pipe = PipelineTransform::new(vec![
            Box::new(MapTransform::from_params(&{
                let mut p = HashMap::new(); p.insert("op".into(), "uppercase".into()); p
            }).unwrap()),
        ]);
        let out = pipe.transform(tiles).unwrap();
        assert_eq!(out[0].payload, b"HELLO");
    }

    // 15. Large batch
    #[test]
    fn large_batch() {
        let tiles: Vec<TileData> = (0..1000).map(|i| tile(format!("tile{i}").as_bytes(), i as u64)).collect();
        let t = SampleTransform::new(10);
        let out = t.transform(tiles).unwrap();
        assert_eq!(out.len(), 100);
    }

    // 16. Flatten empty
    #[test]
    fn flatten_empty() {
        let t = FlattenTransform::new(b",");
        let out = t.transform(vec![]).unwrap();
        assert!(out.is_empty());
    }

    // 17. Base64 roundtrip via map
    #[test]
    fn map_base64_roundtrip() {
        let original = tile(b"hello world", 0);
        let enc_params = { let mut p = HashMap::new(); p.insert("op".into(), "base64_encode".into()); p };
        let enc = MapTransform::from_params(&enc_params).unwrap();
        let encoded = enc.transform(vec![original.clone()]).unwrap();
        let dec_params = { let mut p = HashMap::new(); p.insert("op".into(), "base64_decode".into()); p };
        let dec = MapTransform::from_params(&dec_params).unwrap();
        let decoded = dec.transform(encoded).unwrap();
        assert_eq!(decoded[0].payload, b"hello world");
    }

    // 18. Dedup preserves order
    #[test]
    fn dedup_preserves_order() {
        let tiles = vec![tile(b"x", 0), tile(b"y", 1), tile(b"x", 2), tile(b"z", 3), tile(b"y", 4)];
        let out = DeduplicateTransform.transform(tiles).unwrap();
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].payload, b"x");
        assert_eq!(out[1].payload, b"y");
        assert_eq!(out[2].payload, b"z");
    }
}
