# forge-transform

Tile transform library for Plato agents.

Agents transform tiles. This crate provides the transform primitives — composable, trackable, and serializable.

## Core Types

- **`TileData`** — A unit of data with id, kind, payload, index, and metadata.
- **`TileTransform`** — Trait for transforms: `name()`, `transform()`, `conservation_ratio()`.
- **`TransformSpec`** — Serializable transform specification (name + params).
- **`TransformChain`** — Executes transforms in sequence, tracking conservation ratio at each stage.

## Built-in Transforms

| Transform | Description | Params |
|-----------|-------------|--------|
| `FilterTransform` | Remove tiles by meta field comparison | `field`, `op` (eq/ne/gt/lt/contains), `value` |
| `SortTransform` | Sort tiles by meta field | `field`, `order` (asc/desc) |
| `MapTransform` | Byte ops on payloads | `op` (uppercase/lowercase/trim/reverse/base64_encode/base64_decode) |
| `ChunkTransform` | Group tiles into chunks of N | `size` |
| `FlattenTransform` | Merge tiles (concat payloads) | `separator` |
| `DeduplicateTransform` | Remove tiles with duplicate payloads | — |
| `HeadTransform` | Take first N tiles | `count` |
| `TailTransform` | Take last N tiles | `count` |
| `SampleTransform` | Take every Nth tile | `rate` |
| `PipelineTransform` | Chain multiple transforms | — |

## Usage

```rust
use forge_transform::*;

let tiles = vec![
    TileData::new("text", b"hello".to_vec(), 0).with_meta("x", "1"),
    TileData::new("text", b"world".to_vec(), 1).with_meta("x", "2"),
];

let chain = TransformChain::new(vec![
    Box::new(FilterTransform::new("x", "eq", "1").unwrap()),
    Box::new(MapTransform::from_params(&{
        let mut p = std::collections::HashMap::new();
        p.insert("op".into(), "uppercase".into());
        p
    }).unwrap()),
]);

let (result, stages) = chain.execute(tiles).unwrap();
// result[0].payload == b"HELLO"
// stages track conservation_ratio at each step
```

## Dependencies

- `serde` + `serde_json` — serialization
- `uuid` — tile identifiers

No external transform or error handling crates.

## License

MIT
