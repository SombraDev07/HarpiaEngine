//! Scene file + ECS spawn. GPU meshes stay in `harpia-scene`.

use std::collections::HashMap;
use std::path::Path;

use bevy_ecs::prelude::*;
use bevy_ecs::reflect::ReflectComponent;
use bevy_reflect::Reflect;
use harpia_math::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable id in the `.scene` file. Not an `Entity` — those die on reload.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AuthoringId(pub Uuid);

impl AuthoringId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for AuthoringId {
    fn default() -> Self {
        Self::new()
    }
}

/// Path on disk (`meshes/house.gltf`). Not a `Buffer`.
#[derive(Component, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct AssetPath(pub String);

#[derive(Component, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct MeshRef(pub String);

#[derive(Component, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Reflect)]
#[reflect(Component)]
pub struct MaterialRef(pub String);

/// Instance of a prefab asset. Nested prefabs are E2.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PrefabRef(pub Uuid);

/// Which submit path the cook should emit (Swarm S1). The renderer does not guess.
#[derive(
    Component, Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, Reflect,
)]
#[reflect(Component)]
pub enum RuntimePath {
    /// Gameplay / selection / overrides — CPU draw.
    Cpu,
    /// Static / scatter / vegetation — GPU instance.
    #[default]
    Gpu,
}

/// Local TRS. World matrix is derived; we do not store GPU `WorldTransform` here.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LocalTransform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Default for LocalTransform {
    fn default() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

impl LocalTransform {
    pub fn to_matrix(self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

/// One node in the authoring file. `parent` is a UUID, not an `Entity`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AuthoringNode {
    pub id: Uuid,
    pub name: String,
    pub parent: Option<Uuid>,
    pub local: LocalTransform,
    pub runtime: RuntimePath,
    pub mesh: Option<String>,
    pub material: Option<String>,
    pub prefab: Option<Uuid>,
}

impl AuthoringNode {
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            parent: None,
            local: LocalTransform::default(),
            runtime: RuntimePath::Gpu,
            mesh: None,
            material: None,
            prefab: None,
        }
    }
}

/// The `.scene` document. Text (RON). Cooked binary is E6 (`postcard`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AuthoringScene {
    pub nodes: Vec<AuthoringNode>,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthoringError {
    #[error("{0}")]
    Message(String),
    #[error("ron serialize: {0}")]
    RonSer(#[from] ron::Error),
    #[error("ron parse: {0}")]
    RonDe(#[from] ron::de::SpannedError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("unknown parent {0} on node {1}")]
    MissingParent(Uuid, Uuid),
}

impl AuthoringScene {
    pub fn to_ron(&self) -> Result<String, AuthoringError> {
        let pretty = ron::ser::PrettyConfig::new()
            .struct_names(true)
            .indentor("  ");
        Ok(ron::ser::to_string_pretty(self, pretty)?)
    }

    pub fn from_ron(text: &str) -> Result<Self, AuthoringError> {
        Ok(ron::de::from_str(text)?)
    }

    pub fn write_path(&self, path: &Path) -> Result<(), AuthoringError> {
        std::fs::write(path, self.to_ron()?)?;
        Ok(())
    }

    pub fn read_path(path: &Path) -> Result<Self, AuthoringError> {
        Self::from_ron(&std::fs::read_to_string(path)?)
    }
}

/// Spawn authoring components. Does **not** create `harpia_scene::Mesh` GPU handles.
pub fn spawn_authoring(world: &mut World, scene: &AuthoringScene) -> Result<(), AuthoringError> {
    let mut by_id: HashMap<Uuid, Entity> = HashMap::with_capacity(scene.nodes.len());
    for node in &scene.nodes {
        let mut e = world.spawn((
            AuthoringId(node.id),
            Name::new(node.name.clone()),
            node.local,
            node.runtime,
        ));
        if let Some(path) = &node.mesh {
            e.insert(MeshRef(path.clone()));
        }
        if let Some(path) = &node.material {
            e.insert(MaterialRef(path.clone()));
        }
        if let Some(prefab) = node.prefab {
            e.insert(PrefabRef(prefab));
        }
        by_id.insert(node.id, e.id());
    }
    for node in &scene.nodes {
        let Some(parent_id) = node.parent else {
            continue;
        };
        let child = *by_id
            .get(&node.id)
            .ok_or_else(|| AuthoringError::Message(format!("missing node {}", node.id)))?;
        let parent = *by_id
            .get(&parent_id)
            .ok_or(AuthoringError::MissingParent(parent_id, node.id))?;
        world.entity_mut(child).insert(ChildOf(parent));
    }
    Ok(())
}

/// Rebuild the document from the ECS. Unknown `ChildOf` parents are dropped.
pub fn extract_authoring(world: &mut World) -> AuthoringScene {
    let mut q = world.query::<(
        &AuthoringId,
        &Name,
        &LocalTransform,
        &RuntimePath,
        Option<&MeshRef>,
        Option<&MaterialRef>,
        Option<&PrefabRef>,
        Option<&ChildOf>,
    )>();
    let mut rows = Vec::new();
    for (id, name, local, runtime, mesh, material, prefab, child_of) in q.iter(world) {
        rows.push((
            id.0,
            name.as_str().to_owned(),
            *local,
            *runtime,
            mesh.map(|m| m.0.clone()),
            material.map(|m| m.0.clone()),
            prefab.map(|p| p.0),
            child_of.map(|c| c.parent()),
        ));
    }
    let mut nodes = Vec::with_capacity(rows.len());
    for (id, name, local, runtime, mesh, material, prefab, parent_entity) in rows {
        let parent = parent_entity.and_then(|e| world.get::<AuthoringId>(e).map(|p| p.0));
        nodes.push(AuthoringNode {
            id,
            name,
            parent,
            local,
            runtime,
            mesh,
            material,
            prefab,
        });
    }
    AuthoringScene { nodes }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn house_with_door() -> AuthoringScene {
        let mut house = AuthoringNode::named("House");
        house.mesh = Some("meshes/house.gltf".into());
        house.material = Some("materials/plaster.ron".into());
        let mut door = AuthoringNode::named("Door");
        door.parent = Some(house.id);
        door.mesh = Some("meshes/door.gltf".into());
        door.runtime = RuntimePath::Cpu;
        AuthoringScene {
            nodes: vec![house, door],
        }
    }

    #[test]
    fn ron_roundtrip_keeps_hierarchy_and_paths() {
        let scene = house_with_door();
        let text = scene.to_ron().unwrap();
        let loaded = AuthoringScene::from_ron(&text).unwrap();
        assert_eq!(loaded, scene);
        assert!(
            !text.contains("vb") && !text.contains("Buffer"),
            "authoring file must not store GPU handles:\n{text}"
        );
        assert!(text.contains("meshes/house.gltf"));
        assert!(text.contains("House"));
        assert!(!text.contains("Entity("));
    }

    #[test]
    fn spawn_uses_child_of_not_a_parallel_graph() {
        let scene = house_with_door();
        let mut world = World::new();
        spawn_authoring(&mut world, &scene).unwrap();

        let mut names = world.query::<(&Name, Option<&ChildOf>, &AuthoringId)>();
        let mut door_parent = None;
        let mut house_entity = None;
        let mut house_id = None;
        for (name, child_of, id) in names.iter(&world) {
            if name.as_str() == "House" {
                house_entity = Some(child_of.is_none());
                house_id = Some(id.0);
            }
            if name.as_str() == "Door" {
                door_parent = child_of.map(|c| c.parent());
            }
        }
        assert_eq!(house_entity, Some(true), "house is a root");
        let door_parent = door_parent.expect("door has ChildOf");
        let parent_id = world.get::<AuthoringId>(door_parent).unwrap().0;
        assert_eq!(Some(parent_id), house_id);

        let kids = world
            .get::<Children>(door_parent)
            .expect("Children on house");
        assert_eq!(kids.len(), 1);

        let extracted = extract_authoring(&mut world);
        assert_eq!(extracted.nodes.len(), 2);
        let door = extracted.nodes.iter().find(|n| n.name == "Door").unwrap();
        assert_eq!(door.parent, house_id);
        assert_eq!(door.runtime, RuntimePath::Cpu);
    }

    #[test]
    fn runtime_path_is_reflected() {
        let value: &dyn bevy_reflect::Reflect = &RuntimePath::Gpu;
        assert_eq!(value.reflect_type_ident(), Some("RuntimePath"));
    }

    #[test]
    fn missing_parent_is_an_error() {
        let mut door = AuthoringNode::named("Door");
        door.parent = Some(Uuid::nil());
        let scene = AuthoringScene { nodes: vec![door] };
        let mut world = World::new();
        let err = spawn_authoring(&mut world, &scene).unwrap_err();
        assert!(matches!(err, AuthoringError::MissingParent(_, _)));
    }

    #[test]
    fn file_roundtrip() {
        let dir = std::env::temp_dir().join("harpia-authoring-p");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("house.scene");
        let scene = house_with_door();
        scene.write_path(&path).unwrap();
        let loaded = AuthoringScene::read_path(&path).unwrap();
        assert_eq!(loaded, scene);
    }
}
