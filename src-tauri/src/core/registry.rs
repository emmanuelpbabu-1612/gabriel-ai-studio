use std::collections::HashMap;
use std::time::Instant;

use crate::types::{ModelSpec, Residency};

use super::engine::ModelHandle;

#[derive(Debug)]
pub struct ModelEntry {
    pub spec: ModelSpec,
    pub residency: Residency,
    pub handle: Option<ModelHandle>,
    pub loaded_at: Instant,
    pub last_used: Instant,
}

impl ModelEntry {
    fn new(spec: ModelSpec) -> Self {
        let now = Instant::now();
        Self {
            spec,
            residency: Residency::Cpu,
            handle: None,
            loaded_at: now,
            last_used: now,
        }
    }
}

#[derive(Debug, Default)]
pub struct Registry {
    models: HashMap<String, ModelEntry>,
}

impl Registry {
    pub fn get(&self, id: &str) -> Option<&ModelEntry> {
        self.models.get(id)
    }

    pub fn find_matching<'a>(
        &'a self,
        id: &str,
        model_type: crate::types::ModelType,
    ) -> Option<(&'a str, &'a ModelEntry)> {
        if let Some(entry) = self.models.get(id) {
            if entry.spec.model_type == model_type {
                return Some((entry.spec.id.as_str(), entry));
            }
        }

        let matching_entries: Vec<(&String, &ModelEntry)> = self
            .models
            .iter()
            .filter(|(k, e)| {
                e.spec.model_type == model_type
                    && (k.as_str() == id
                        || k.starts_with(id)
                        || id.starts_with(k.as_str())
                        || k.contains(id)
                        || id.contains(k.as_str()))
            })
            .collect();

        if let Some((k, e)) = matching_entries.first() {
            return Some((k.as_str(), e));
        }

        let type_entries: Vec<(&String, &ModelEntry)> = self
            .models
            .iter()
            .filter(|(_, e)| e.spec.model_type == model_type)
            .collect();
        if type_entries.len() == 1 {
            let (k, e) = type_entries[0];
            return Some((k.as_str(), e));
        }

        None
    }

    pub fn contains(&self, id: &str) -> bool {
        self.models.contains_key(id)
    }

    pub fn insert_new(&mut self, spec: ModelSpec) {
        self.models.insert(spec.id.clone(), ModelEntry::new(spec));
    }

    pub fn remove(&mut self, id: &str) -> Option<ModelEntry> {
        self.models.remove(id)
    }

    pub fn promote(&mut self, id: &str, handle: ModelHandle) {
        if let Some(e) = self.models.get_mut(id) {
            e.residency = Residency::Gpu;
            e.handle = Some(handle);
            e.last_used = Instant::now();
        }
    }

    pub fn demote(&mut self, id: &str) {
        if let Some(e) = self.models.get_mut(id) {
            e.residency = Residency::Cpu;
            e.handle = None;
            e.last_used = Instant::now();
        }
    }

    pub fn touch(&mut self, id: &str) {
        if let Some(e) = self.models.get_mut(id) {
            e.last_used = Instant::now();
        }
    }

    pub fn resident_ids(&self) -> Vec<String> {
        self.models
            .iter()
            .filter(|(_, e)| e.residency == Residency::Gpu && e.handle.is_some())
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub fn least_recently_used_resident(&self) -> Option<String> {
        self.models
            .iter()
            .filter(|(_, e)| e.residency == Residency::Gpu && e.handle.is_some())
            .min_by_key(|(_, e)| e.last_used)
            .map(|(id, _)| id.clone())
    }

    pub fn len(&self) -> usize {
        self.models.len()
    }

    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }

    /// Oldest entry of any residency whose idle time exceeds `min_age`,
    /// eligible for full eviction when the slot pool is saturated.
    pub fn least_recently_used_any_idle(&self, min_age: std::time::Duration) -> Option<String> {
        let cutoff = Instant::now() - min_age;
        self.models
            .iter()
            .filter(|(_, e)| e.last_used < cutoff)
            .min_by_key(|(_, e)| e.last_used)
            .map(|(id, _)| id.clone())
    }

    pub fn evictable_idle(&self, older_than: std::time::Duration) -> Vec<String> {
        let cutoff = Instant::now() - older_than;
        let mut ids: Vec<(Instant, String)> = self
            .models
            .iter()
            .filter(|(_, e)| e.residency == Residency::Gpu && e.handle.is_some() && e.last_used < cutoff)
            .map(|(id, e)| (e.last_used, id.clone()))
            .collect();
        ids.sort_by_key(|(t, _)| *t);
        ids.into_iter().map(|(_, id)| id).collect()
    }

    pub fn snapshot(&self) -> Vec<crate::types::ModelRuntimeInfo> {
        let now = Instant::now();
        self.models
            .values()
            .map(|e| crate::types::ModelRuntimeInfo {
                id: e.spec.id.clone(),
                model_type: e.spec.model_type,
                residency: e.residency,
                vram_bytes: match e.residency {
                    Residency::Gpu => e.spec.vram_bytes,
                    Residency::Cpu => 0,
                },
                idle_secs: now.duration_since(e.last_used).as_secs(),
            })
            .collect()
    }
}
