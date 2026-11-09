//! Opaque handles owned by wasm.
//!
//! None has a public constructor; JS releases them with `free()` or
//! `Symbol.dispose`. A selection shares ownership of its immutable manifest, so
//! it outlives the manifest handle, and each plan owns everything it needs, so
//! it outlives its selection. Every `value` read returns a fresh plain object.

use std::rc::Rc;

use manifest_metadata::SupportedPlatform;
use wasm_bindgen::prelude::*;

use crate::bridge::encode;

/// A manifest that passed semantic validation.
#[wasm_bindgen]
#[derive(Debug)]
#[must_use]
pub struct ValidatedManifest {
    pub(crate) inner: Rc<manifest_core::ValidatedManifest>,
}

#[wasm_bindgen]
impl ValidatedManifest {
    /// The validated manifest.
    #[wasm_bindgen(getter, unchecked_return_type = "ResourceManifest")]
    pub fn value(&self) -> Result<JsValue, JsValue> {
        encode(self.inner.manifest())
    }

    /// Whether the manifest declares a newer minor version than this runtime.
    #[wasm_bindgen(getter, js_name = newerMinor)]
    #[must_use]
    pub fn newer_minor(&self) -> bool {
        self.inner.newer_minor()
    }
}

/// The artifact of one resource variant for one platform.
#[wasm_bindgen]
#[derive(Debug)]
#[must_use]
pub struct SelectedArtifact {
    manifest: Rc<manifest_core::ValidatedManifest>,
    resource: String,
    variant: String,
    platform: SupportedPlatform,
}

impl SelectedArtifact {
    pub(crate) fn new(
        manifest: &Rc<manifest_core::ValidatedManifest>,
        resource: String,
        variant: String,
        platform: SupportedPlatform,
    ) -> Self {
        Self {
            manifest: Rc::clone(manifest),
            resource,
            variant,
            platform,
        }
    }

    pub(crate) fn selected(&self) -> manifest_core::SelectedArtifact<'_> {
        self.manifest
            .artifact(&self.resource, &self.variant, self.platform)
            .expect("selected artifact belongs to its immutable validated manifest")
    }
}

#[wasm_bindgen]
impl SelectedArtifact {
    /// The selected variant.
    #[wasm_bindgen(getter, unchecked_return_type = "ResourceVariant")]
    pub fn value(&self) -> Result<JsValue, JsValue> {
        encode(self.selected().variant())
    }
}

/// How to resolve the version of a selected artifact.
#[wasm_bindgen]
#[derive(Debug)]
#[must_use]
pub struct VersionPlan {
    pub(crate) inner: manifest_core::VersionPlan,
}

#[wasm_bindgen]
impl VersionPlan {
    /// The plan's wire projection.
    #[wasm_bindgen(getter, unchecked_return_type = "VersionPlanValue")]
    pub fn value(&self) -> Result<JsValue, JsValue> {
        encode(&self.inner)
    }
}

/// How to download and verify a selected artifact at one version.
#[wasm_bindgen]
#[derive(Debug)]
#[must_use]
pub struct DownloadPlan {
    pub(crate) inner: manifest_core::DownloadPlan,
}

#[wasm_bindgen]
impl DownloadPlan {
    /// The plan's wire projection.
    #[wasm_bindgen(getter, unchecked_return_type = "DownloadPlanValue")]
    pub fn value(&self) -> Result<JsValue, JsValue> {
        encode(&self.inner)
    }
}
