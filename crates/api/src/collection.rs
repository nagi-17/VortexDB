use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use defs::{Dimension, Similarity};
use index::IndexType;

use crate::{ApiError, DbConfig, Result, VectorDb, init_api};

pub const DEFAULT_COLLECTION: &str = "default";
const MAX_COLLECTION_NAME_LEN: usize = 64;

#[derive(Debug, Clone)]
pub struct CollectionSpec {
    pub id: String,
    pub dimension: Dimension,
    pub similarity: Similarity,
    pub index_type: IndexType,
}

pub struct CollectionManager {
    root: PathBuf, // DATA_PATH
    defaults: DbConfig,
    collections: RwLock<HashMap<String, Arc<VectorDb>>>,
    specs: RwLock<HashMap<String, CollectionSpec>>,
}

// TODO : rethink how to implement "default" collection (whether to have a 'default' collection or not)
impl CollectionManager {
    pub fn new(root: PathBuf, defaults: DbConfig) -> Result<Self> {
        let collections_root = root.join("collections");

        fs::create_dir_all(&collections_root).map_err(|e| ApiError::InitializationFailed {
            reason: format!(
                "could not create collections dir {}: {e}",
                collections_root.display()
            ),
        })?;

        let default_cfg = DbConfig {
            data_path: collections_root.join(DEFAULT_COLLECTION),
            ..defaults.clone()
        };

        let default_db = init_api(default_cfg)?;

        let default_spec = CollectionSpec {
            id: DEFAULT_COLLECTION.to_string(),
            dimension: defaults.dimension,
            similarity: defaults.similarity,
            index_type: defaults.index_type,
        };

        Ok(Self {
            root,
            defaults,
            collections: RwLock::new(HashMap::from([(
                DEFAULT_COLLECTION.to_string(),
                Arc::new(default_db),
            )])),
            specs: RwLock::new(HashMap::from([(
                DEFAULT_COLLECTION.to_string(),
                default_spec,
            )])),
        })
    }

    pub fn get(&self, name: &str) -> Result<Arc<VectorDb>> {
        let key = Self::resolve(name);

        let collections = self.collections.read().map_err(|_| ApiError::LockError)?;

        collections
            .get(key)
            .cloned()
            .ok_or_else(|| ApiError::CollectionNotFound {
                name: key.to_string(),
            })
    }

    fn resolve(name: &str) -> &str {
        if name.is_empty() {
            DEFAULT_COLLECTION
        } else {
            name
        }
    }

    pub fn create(&self, spec: CollectionSpec) -> Result<Arc<VectorDb>> {
        validate_name(&spec.id)?;

        // dimension check
        if spec.dimension == 0 {
            return Err(ApiError::InitializationFailed {
                reason: "collection dimension must be greater than zero".to_string(),
            });
        }

        // Check for existing dir/collection of the same name in root/collections dir
        // (as the 'collections' HashMap may not contain the collection which have been created during previous startups)
        // This may lead to opening of existing RocksDB and the new collection (acc. to the user)
        // would inherit the points of the already exisiting collection
        let path = self.root.join("collections").join(&spec.id);
        if path.exists() {
            return Err(ApiError::CollectionAlreadyExists {
                name: spec.id.clone(),
            });
        }

        // load/build config
        let cfg = DbConfig {
            data_path: path,
            dimension: spec.dimension,
            similarity: spec.similarity,
            index_type: spec.index_type,
            ..self.defaults.clone()
        };

        let db = {
            let mut collections = self.collections.write().map_err(|_| ApiError::LockError)?;

            if collections.contains_key(&spec.id) {
                return Err(ApiError::CollectionAlreadyExists {
                    name: spec.id.clone(),
                });
            }

            let db = Arc::new(init_api(cfg)?);
            collections.insert(spec.id.clone(), Arc::clone(&db));
            db
        };

        let mut specs = self.specs.write().map_err(|_| ApiError::LockError)?;
        specs.insert(spec.id.clone(), spec);

        Ok(db)
    }

    pub fn list(&self) -> Result<Vec<CollectionSpec>> {
        let mut specs = {
            let specs = self.specs.read().map_err(|_| ApiError::LockError)?;
            specs.values().cloned().collect::<Vec<_>>()
        };

        // random thing added : sort HashMap for better visiblitiy for the usr
        specs.sort_by(|a, b| a.id.cmp(&b.id));

        Ok(specs)
    }
}

fn validate_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name.len() <= MAX_COLLECTION_NAME_LEN
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');

    if valid {
        Ok(())
    } else {
        Err(ApiError::InvalidCollectionName {
            name: name.to_string(),
        })
    }
}
