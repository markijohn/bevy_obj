#[cfg_attr(not(feature = "scene"), path = "mesh.rs")]
#[cfg_attr(feature = "scene", path = "scene.rs")]
mod load_impl;
pub use load_impl::*;

use bevy_asset::{io::Reader, AssetLoader, AsyncReadExt, LoadContext};
use bevy_utils::ConditionalSendFuture;
use bevy_render::{
    mesh::{ MeshVertexAttribute},
    render_resource::{VertexFormat},
};

pub struct ObjLoader;

// Object vertex index
pub const OBJECT_VERTEX_INDEX: MeshVertexAttribute =
    MeshVertexAttribute::new("ObjectVertexIndex", 42424242, VertexFormat::Uint32);


impl AssetLoader for ObjLoader {
    type Error = ObjError;
    type Settings = ();
    type Asset = AssetType;

    fn load<'a>(
        &'a self,
        reader: &'a mut Reader,
        _settings: &'a Self::Settings,
        load_context: &'a mut LoadContext,
    ) -> impl ConditionalSendFuture<Output = Result<Self::Asset, Self::Error>> {
        Box::pin(async move {
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).await?;
            load_obj(&bytes, load_context).await
        })
    }

    fn extensions(&self) -> &[&str] {
        super::EXTENSIONS
    }
}
