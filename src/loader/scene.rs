use bevy_color::Color;
use bevy_asset::{AssetPath, Handle, LoadContext};
use bevy_ecs::world::World;
use bevy_pbr::{PbrBundle, StandardMaterial};
use bevy_render::{
    mesh::{Indices, Mesh},
    render_asset::RenderAssetUsages,
    render_resource::PrimitiveTopology,
    texture::Image,
};
use bevy_scene::Scene;
use std::path::PathBuf;
use bevy_render::alpha::AlphaMode;
use thiserror::Error;

pub type AssetType = Scene;

fn material_label(idx: usize) -> String {
    "Material".to_owned() + &idx.to_string()
}

fn mesh_label(idx: usize) -> String {
    "Mesh".to_owned() + &idx.to_string()
}

#[derive(Error, Debug)]
pub enum ObjError {
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Invalid OBJ file: {0}")]
    TobjError(#[from] tobj::LoadError),
    #[error("Failed to load materials for {0}: {1}")]
    MaterialError(PathBuf, #[source] tobj::LoadError),
    #[error("Invalid image file for texture: {0}")]
    InvalidImageFile(PathBuf),
    #[error("Asset reading failed: {0}")]
    AssetLoadError(#[from] bevy_asset::AssetLoadError),
    #[error("Texture conversion failed: {0}")]
    TextureError(#[from] bevy_render::texture::TextureError),
}

pub(super) async fn load_obj<'a, 'b>(
    bytes: &'a [u8],
    load_context: &'a mut LoadContext<'b>,
) -> Result<Scene, ObjError> {
    load_obj_scene(bytes, load_context).await
}

async fn load_obj_data<'a, 'b>(
    mut bytes: &'a [u8],
    load_context: &'a mut LoadContext<'b>,
) -> tobj::LoadResult {
    let options = tobj::GPU_LOAD_OPTIONS;
    tobj::load_obj_buf_async(&mut bytes, &options, |p| async {
        // We don't use the MTL material as an asset, just load the bytes of it.
        // But we are unable to call ctx.finish() and feed the result back. (which is no new asset)
        // Is this allowed?
        let mut ctx = load_context.begin_labeled_asset();
        let path = PathBuf::from(ctx.asset_path().to_string()).with_file_name(p);
        let asset_path = AssetPath::from(path.to_string_lossy().into_owned());

        ctx.read_asset_bytes(&asset_path)
            .await
            .map_or(Err(tobj::LoadError::OpenFileFailed), |bytes| {
                tobj::load_mtl_buf(&mut bytes.as_slice())
            })
    })
    .await
}

fn load_mat_texture(
    texture: &Option<String>,
    load_context: &mut LoadContext,
) -> Option<Handle<Image>> {
    if let Some(texture) = texture {
        //@madtek : texture path wrapped double quote
        let texture = if let (Some(s), Some(e)) = ( texture.find('"'), texture.rfind('\"') ) {
            &texture[s+1 .. e]
        } else {
            texture.as_str()
        };

        let path = PathBuf::from(load_context.asset_path().to_string()).with_file_name(texture);
        let asset_path = AssetPath::from(path.to_string_lossy().into_owned());
        Some(load_context.load(&asset_path))
    } else {
        None
    }
}

async fn load_obj_scene<'a, 'b>(
    bytes: &'a [u8],
    load_context: &'a mut LoadContext<'b>,
) -> Result<Scene, ObjError> {
    let (models, materials) = load_obj_data(bytes, load_context).await?;
    let materials = materials.map_err(|err| {
        let obj_path = load_context.path().to_path_buf();
        ObjError::MaterialError(obj_path, err)
    })?;

    let mut mat_handles = Vec::with_capacity(materials.len());
    for (mat_idx, mat) in materials.into_iter().enumerate() {
        let mut material = StandardMaterial {
            base_color_texture: load_mat_texture(&mat.diffuse_texture, load_context),
            normal_map_texture: load_mat_texture(&mat.normal_texture, load_context),
            emissive_texture: load_mat_texture(&mat.ambient_texture, load_context),
            ..Default::default()
        };

        if mat.diffuse_texture.as_ref().map( |s| s.as_str() ).unwrap_or("").find("Eye").is_some()
        || mat.name.find("Eye").is_some() {
            println!("{:#?}", mat);
        }
        // if let Some(color) = mat.diffuse {
        //     material.base_color = Color::srgb(color[0], color[1], color[2]);
        // }
        let diffuse = mat.diffuse.unwrap_or( [1.0, 1.0, 1.0] );
        let alpha = mat.dissolve.unwrap_or(1.0);
        if let Some(ambient) = mat.ambient {
            material.base_color = Color::srgba(
                diffuse[0] * ambient[0],
                diffuse[1] * ambient[1],
                diffuse[2] * ambient[2],
                alpha
            );
        } else {
            material.base_color = Color::srgba(diffuse[0], diffuse[1], diffuse[2], alpha );
        };
        material.perceptual_roughness = mat.shininess.map_or(1.0, |shininess| {
            // shininess의 범위가 보통 0~1000이므로 반비례하여 roughness로 변환
            //(1.0 - (shininess / 1000.0)).clamp(0.089, 1.0)
            (1.0 - shininess).clamp(0.089, 1.0)
        });
        if alpha != 1.0 {
            material.alpha_mode = AlphaMode::Blend;
        }

        material.reflectance = mat.specular.map_or(0.5, |specular| {
            (specular[0] + specular[1] + specular[2]) / 3.0
        });

        mat_handles.push(load_context.add_labeled_asset(material_label(mat_idx), material));
    }

    let mut world = World::default();
    for (model_idx, model) in models.into_iter().enumerate() {
        let vertex_position: Vec<[f32; 3]> = model
            .mesh
            .positions
            .chunks_exact(3)
            .map(|v| [v[0], v[1], v[2]])
            .collect();
        let vertex_normal: Vec<[f32; 3]> = model
            .mesh
            .normals
            .chunks_exact(3)
            .map(|n| [n[0], n[1], n[2]])
            .collect();
        let vertex_texture: Vec<[f32; 2]> = model
            .mesh
            .texcoords
            .chunks_exact(2)
            .map(|t| [t[0], 1.0 - t[1]])
            .collect();

        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_indices(Indices::U32(model.mesh.indices));

        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vertex_position);
        if !vertex_texture.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vertex_texture);
        }

        if !vertex_normal.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vertex_normal);
        } else {
            mesh.duplicate_vertices();
            mesh.compute_flat_normals();
        }

        let mesh_handle = load_context.add_labeled_asset(mesh_label(model_idx), mesh);
        //println!("{}", model.name);
        // let mesh_handle = load_context.add_labeled_asset(model.name, mesh);

        let mut pbr_bundle = PbrBundle {
            mesh: mesh_handle,
            ..Default::default()
        };
        // Now assign the material, if present
        if let Some(mat_id) = model.mesh.material_id {
            pbr_bundle.material = mat_handles[mat_id].clone();
        }
        world.spawn(pbr_bundle);
    }

    Ok(Scene::new(world))
}
