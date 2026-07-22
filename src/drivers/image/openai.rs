//! OpenAI Images API backend (`image:openai`).
//!
//! Request/endpoint contracts are mirrored in `docs/downstream-contracts.md`
//! (created in Task 8); any change here must update that file in the same
//! commit.

use serde::Deserialize;

use crate::error::{DriverError, ErrorKind};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GenerateArgs {
    pub prompt: String,
    pub n: Option<u32>,
    pub size: Option<String>,
    pub quality: Option<String>,
    pub refs: Option<Vec<String>>,
    pub out_dir: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EditArgs {
    pub input: String,
    pub prompt: String,
    pub mask: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ImagesResponse {
    data: Vec<ImageDatum>,
}

#[derive(Debug, Deserialize)]
struct ImageDatum {
    b64_json: Option<String>,
}

/// JSON body for POST /images/generations. Optional fields are omitted when
/// absent (never sent as null) so golden-request tests pin the exact wire
/// shape.
pub(crate) fn generation_body(model: &str, a: &GenerateArgs) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    m.insert("model".into(), model.into());
    m.insert("prompt".into(), a.prompt.clone().into());
    if let Some(n) = a.n {
        m.insert("n".into(), n.into());
    }
    if let Some(s) = &a.size {
        m.insert("size".into(), s.clone().into());
    }
    if let Some(q) = &a.quality {
        m.insert("quality".into(), q.clone().into());
    }
    serde_json::Value::Object(m)
}

/// `2026-07-22T10:15:30Z` → `20260722T101530Z-<n>.png` (spec §6: files are
/// named `<UTC-ts>-<n>.png`).
pub(crate) fn image_filename(rfc3339: &str, n: usize) -> String {
    format!("{}-{}.png", rfc3339.replace(['-', ':'], ""), n)
}

pub(crate) fn decode_b64_png(s: &str) -> Result<Vec<u8>, DriverError> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| {
            DriverError::new(
                ErrorKind::Upstream,
                format!("invalid base64 image data: {e}"),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_body_includes_all_present_fields() {
        let a = GenerateArgs {
            prompt: "a red square".into(),
            n: Some(2),
            size: Some("1024x1024".into()),
            quality: Some("high".into()),
            refs: None,
            out_dir: None,
        };
        assert_eq!(
            generation_body("gpt-image-1", &a),
            serde_json::json!({
                "model": "gpt-image-1",
                "prompt": "a red square",
                "n": 2,
                "size": "1024x1024",
                "quality": "high"
            })
        );
    }

    #[test]
    fn generation_body_omits_absent_fields() {
        let a = GenerateArgs {
            prompt: "p".into(),
            n: None,
            size: None,
            quality: None,
            refs: None,
            out_dir: None,
        };
        assert_eq!(
            generation_body("gpt-image-1", &a),
            serde_json::json!({"model": "gpt-image-1", "prompt": "p"})
        );
    }

    #[test]
    fn filename_compacts_timestamp() {
        assert_eq!(
            image_filename("2026-07-22T10:15:30Z", 1),
            "20260722T101530Z-1.png"
        );
    }

    #[test]
    fn args_reject_unknown_fields() {
        let e = serde_json::from_value::<GenerateArgs>(
            serde_json::json!({"prompt": "p", "promt_typo": 1}),
        )
        .err()
        .unwrap();
        assert!(e.to_string().contains("promt_typo"));
    }

    #[test]
    fn b64_decode_maps_to_upstream() {
        let e = decode_b64_png("!!!not-base64!!!").err().unwrap();
        assert!(matches!(e.kind, ErrorKind::Upstream));
        use base64::Engine as _;
        let ok = decode_b64_png(&base64::engine::general_purpose::STANDARD.encode(b"png-bytes"))
            .unwrap();
        assert_eq!(ok, b"png-bytes");
    }
}
