use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Arm {
    pub visits: u64,
    pub mean_reward: f64,
}

/// Contextual epsilon-greedy bandit over three heuristic styles and two hand strengths.
/// Rewards are final seat scores / 180. This adapts style selection, not card-level policy.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Learning {
    pub version: u32,
    pub rounds: u64,
    pub arms: [[Arm; 3]; 2],
}

impl Default for Learning {
    fn default() -> Self {
        Self {
            version: 1,
            rounds: 0,
            arms: Default::default(),
        }
    }
}

impl Learning {
    pub fn load(path: &Path) -> io::Result<Self> {
        match fs::read(path) {
            Ok(bytes) => {
                let state: Self = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
                if state.version != 1
                    || state
                        .arms
                        .iter()
                        .flatten()
                        .any(|a| !a.mean_reward.is_finite() || a.mean_reward.abs() > 1.0)
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Unsupported or invalid learning state",
                    ));
                }
                Ok(state)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    pub fn select(&self, context: usize, rng: &mut impl Rng) -> usize {
        let arms = &self.arms[context];
        if let Some(index) = arms.iter().position(|arm| arm.visits == 0) {
            return index;
        }
        if rng.random_bool(0.12) {
            return rng.random_range(0..3);
        }
        (0..3)
            .max_by(|a, b| arms[*a].mean_reward.total_cmp(&arms[*b].mean_reward))
            .unwrap()
    }

    pub fn observe(&mut self, context: usize, style: usize, score: i32) {
        let arm = &mut self.arms[context][style];
        arm.visits += 1;
        arm.mean_reward += (f64::from(score) / 180.0 - arm.mean_reward) / arm.visits as f64;
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
        let bytes = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        let mut file = fs::File::create(&temporary)?;
        use io::Write;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(temporary, path)
    }
}

pub fn default_path() -> PathBuf {
    if let Some(path) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(path).join("koor/learning.json");
    }
    if cfg!(target_os = "windows")
        && let Some(path) = std::env::var_os("LOCALAPPDATA")
    {
        return PathBuf::from(path).join("Koor/learning.json");
    }
    if let Some(path) = std::env::var_os("HOME") {
        let directory = if cfg!(target_os = "macos") {
            "Library/Application Support"
        } else {
            ".local/share"
        };
        return PathBuf::from(path)
            .join(directory)
            .join("koor/learning.json");
    }
    PathBuf::from("koor-learning.json")
}
