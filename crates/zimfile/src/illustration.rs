//! Illustrations: the archive's icons, stored as `M/Illustration_<w>x<h>@<scale>`
//! with optional `;key=value` attributes (libzim's `IllustrationInfo`).

use std::collections::BTreeMap;
use std::fmt::Write;

#[derive(Debug, Clone, PartialEq)]
pub struct Illustration {
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub attributes: BTreeMap<String, String>,
}

impl Illustration {
    /// A `size`×`size` illustration at scale 1, without attributes.
    pub fn square(size: u32) -> Illustration {
        Illustration {
            width: size,
            height: size,
            scale: 1.0,
            attributes: BTreeMap::new(),
        }
    }

    /// Its metadata name, e.g. `Illustration_48x48@1;ui=dark`.
    // ponytail: libzim prints the scale as C++ streams do (%g, 6 significant
    // digits); Rust prints the shortest round-trip form. They agree on every
    // scale but ones with more than 6 significant digits or tiny exponents.
    pub fn name(&self) -> String {
        let mut s = format!("Illustration_{}x{}@{}", self.width, self.height, self.scale);
        for (k, v) in &self.attributes {
            let _ = write!(s, ";{k}={v}");
        }
        s
    }

    /// Parse a metadata name; `None` unless `name` is exactly what
    /// [`name`](Self::name) prints (no spaces, signs, leading zeros, …).
    pub fn parse(name: &str) -> Option<Illustration> {
        let rest = name.strip_prefix("Illustration_")?;
        let (dims, attrs) = match rest.split_once(';') {
            Some((d, a)) => (d, Some(a)),
            None => (rest, None),
        };
        let (w, rest) = dims.split_once('x')?;
        let (h, scale) = rest.split_once('@')?;
        let attributes = attrs
            .into_iter()
            .flat_map(|a| a.split(';'))
            .map(|kv| {
                let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
                (k.to_string(), v.to_string())
            })
            .collect();
        let i = Illustration {
            width: w.parse().ok()?,
            height: h.parse().ok()?,
            scale: scale.parse().ok()?,
            attributes,
        };
        (i.scale >= 0.0 && i.name() == name).then_some(i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ii(w: u32, h: u32, scale: f32, attrs: &[(&str, &str)]) -> Illustration {
        Illustration {
            width: w,
            height: h,
            scale,
            attributes: attrs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    // libzim test/illustrations.cpp:28 parsingOfValidInput
    #[test]
    fn parsing_of_valid_input() {
        for (s, want) in [
            ("Illustration_0x0@1", ii(0, 0, 1.0, &[])),
            ("Illustration_1x1@1", ii(1, 1, 1.0, &[])),
            ("Illustration_64x64@1", ii(64, 64, 1.0, &[])),
            ("Illustration_64x64@2", ii(64, 64, 2.0, &[])),
            ("Illustration_64x48@2", ii(64, 48, 2.0, &[])),
            (
                "Illustration_64x64@1;ui=dark",
                ii(64, 64, 1.0, &[("ui", "dark")]),
            ),
            (
                "Illustration_48x64@1.5;lang=fr;ui=dark",
                ii(48, 64, 1.5, &[("lang", "fr"), ("ui", "dark")]),
            ),
        ] {
            assert_eq!(Illustration::parse(s), Some(want), "{s}");
        }
    }

    // libzim test/illustrations.cpp:44 parsingOfInvalidInput
    #[test]
    fn parsing_of_invalid_input() {
        for s in [
            "Illustration_01x01@1",
            "Illustration_48x48@1;",
            "Illstration_64x64@1",
            "Illstration_",
            "Illustration_64x@1",
            "Illustration_64x",
            "Illustration_64x64",
            "Illustration_-32x-32@1",
            "Illustration_ 64x64@1",
            "Illustration_64x 64@1",
            "Illustration_ 64x 64@1",
            "Illustration_1 28x1 28@1",
            "Illustration_48x64@1.5000",
            "Illustration_48x64@1.5;ui=dark;lang=fr",
            "Illustration_48x48@-1",
            "Illustration_+48x48@1",
        ] {
            assert_eq!(Illustration::parse(s), None, "{s}");
        }
    }

    // libzim test/illustrations.cpp:68 asMetadataItemName
    #[test]
    fn as_metadata_item_name() {
        assert_eq!(ii(64, 64, 1.0, &[]).name(), "Illustration_64x64@1");
        assert_eq!(ii(64, 64, 2.0, &[]).name(), "Illustration_64x64@2");
        assert_eq!(ii(64, 48, 2.0, &[]).name(), "Illustration_64x48@2");
        assert_eq!(
            ii(64, 48, 2.0, &[("uischeme", "dark")]).name(),
            "Illustration_64x48@2;uischeme=dark"
        );
        assert_eq!(
            ii(
                64,
                48,
                2.0,
                &[("uischeme", "dark"), ("colorscheme", "monochrome")]
            )
            .name(),
            "Illustration_64x48@2;colorscheme=monochrome;uischeme=dark"
        );
        assert_eq!(Illustration::square(48), ii(48, 48, 1.0, &[]));
    }
}
