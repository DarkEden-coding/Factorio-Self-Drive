//! `import fsim`: batched RL environment over the Rust simulator (docs/rl.md,
//! "Python bindings"). Steps run with the GIL released and write straight
//! into freshly allocated NumPy arrays.
use fsim_core::{
    policy_obs::{self, COARSE_SIZE, LOCAL_SIZE, VECTOR_LEN},
    reward::TERMS,
    rl_config::{Mode, RlConfig},
    rl_episode::COLLISION_KINDS,
    vec_env::{self, Actions, ObsBatch, OwnedActions, StepInfo},
    Prototypes,
};
use numpy::{
    ndarray::Dim, PyArray, PyArray1, PyArrayMethods, PyReadonlyArrayDyn, PyReadwriteArray,
    PyUntypedArrayMethods,
};
use pyo3::{
    exceptions::PyValueError,
    prelude::*,
    types::{PyDict, PyTuple},
};
use std::sync::Arc;

const PROTOTYPES: &str = include_str!("../../../data/prototypes.json");
const CHANNELS: usize = fsim_core::observation::CHANNELS.len();

fn value_error(message: impl Into<String>) -> PyErr {
    PyValueError::new_err(message.into())
}

fn from_json<'py>(py: Python<'py>, json: &str) -> PyResult<Bound<'py, PyAny>> {
    py.import("json")?.call_method1("loads", (json,))
}

/// Observation arrays for `n` envs: local u8 [n,8,64,64], coarse u8 [n,8,32,32], vector f32 [n,V]
/// when the config's `observation` includes the full one, and student f32 [n,S] when it includes
/// the student one.
struct ObsArrays<'py> {
    full: Option<FullArrays<'py>>,
    student: Option<Bound<'py, PyArray<f32, Dim<[usize; 2]>>>>,
}

struct FullArrays<'py> {
    local: Bound<'py, PyArray<u8, Dim<[usize; 4]>>>,
    coarse: Bound<'py, PyArray<u8, Dim<[usize; 4]>>>,
    vector: Bound<'py, PyArray<f32, Dim<[usize; 2]>>>,
}

impl<'py> ObsArrays<'py> {
    fn zeros(py: Python<'py>, n: usize, config: &RlConfig) -> Self {
        let kind = config.observation;
        Self {
            full: kind.full().then(|| FullArrays {
                local: PyArray::zeros(py, [n, CHANNELS, LOCAL_SIZE, LOCAL_SIZE], false),
                coarse: PyArray::zeros(py, [n, CHANNELS, COARSE_SIZE, COARSE_SIZE], false),
                vector: PyArray::zeros(py, [n, VECTOR_LEN], false),
            }),
            student: kind
                .student()
                .then(|| PyArray::zeros(py, [n, config.student_len()], false)),
        }
    }

    fn into_dict(self, prefix: &str, dict: &Bound<'py, PyDict>) -> PyResult<()> {
        if let Some(full) = self.full {
            dict.set_item(format!("{prefix}local"), full.local)?;
            dict.set_item(format!("{prefix}coarse"), full.coarse)?;
            dict.set_item(format!("{prefix}vector"), full.vector)?;
        }
        if let Some(student) = self.student {
            dict.set_item(format!("{prefix}student"), student)?;
        }
        Ok(())
    }
}

/// Runs `f` on mutable slices of the arrays with the GIL released. Absent
/// arrays become empty slices.
fn with_obs<'py, R: Send>(
    py: Python<'py>,
    a: &ObsArrays<'py>,
    b: Option<&ObsArrays<'py>>,
    f: impl FnOnce(ObsBatch<'_>, Option<ObsBatch<'_>>) -> R + Send,
) -> PyResult<R> {
    struct Guards<'py> {
        local: Option<PyReadwriteArray<'py, u8, Dim<[usize; 4]>>>,
        coarse: Option<PyReadwriteArray<'py, u8, Dim<[usize; 4]>>>,
        vector: Option<PyReadwriteArray<'py, f32, Dim<[usize; 2]>>>,
        student: Option<PyReadwriteArray<'py, f32, Dim<[usize; 2]>>>,
    }
    fn guards<'py>(a: &ObsArrays<'py>) -> Guards<'py> {
        Guards {
            local: a.full.as_ref().map(|f| f.local.readwrite()),
            coarse: a.full.as_ref().map(|f| f.coarse.readwrite()),
            vector: a.full.as_ref().map(|f| f.vector.readwrite()),
            student: a.student.as_ref().map(|s| s.readwrite()),
        }
    }
    fn slice<'a, T: numpy::Element, D: numpy::ndarray::Dimension>(
        g: &'a mut Option<PyReadwriteArray<'_, T, D>>,
    ) -> PyResult<&'a mut [T]> {
        Ok(match g {
            Some(g) => g.as_slice_mut()?,
            None => &mut [],
        })
    }
    fn batch<'a>(g: &'a mut Guards<'_>) -> PyResult<ObsBatch<'a>> {
        Ok(ObsBatch {
            local: slice(&mut g.local)?,
            coarse: slice(&mut g.coarse)?,
            vector: slice(&mut g.vector)?,
            student: slice(&mut g.student)?,
        })
    }
    let mut first = guards(a);
    let first = batch(&mut first)?;
    match b {
        None => Ok(py.detach(|| f(first, None))),
        Some(b) => {
            let mut second = guards(b);
            let second = batch(&mut second)?;
            Ok(py.detach(|| f(first, Some(second))))
        }
    }
}

/// Batched environment. See docs/rl.md for config, observations and info.
#[pyclass(module = "fsim")]
struct VecEnv {
    inner: vec_env::VecEnv,
}

#[pymethods]
impl VecEnv {
    #[new]
    fn new(py: Python<'_>, config_json: &str, num_envs: usize, seed: u64) -> PyResult<Self> {
        let config = RlConfig::from_json(config_json).map_err(value_error)?;
        let data = Arc::new(Prototypes::from_json(PROTOTYPES).map_err(value_error)?);
        let inner = py
            .detach(|| vec_env::VecEnv::new(config, data, num_envs, seed))
            .map_err(value_error)?;
        Ok(Self { inner })
    }

    #[getter]
    fn num_envs(&self) -> usize {
        self.inner.num_envs()
    }

    #[getter]
    fn mode(&self) -> &'static str {
        self.inner.mode().name()
    }

    /// Pool names of the loaded maps, indexed by `info["map_index"]`'s pool index.
    #[getter]
    fn map_names(&self) -> Vec<(usize, String)> {
        self.inner
            .maps()
            .iter()
            .map(|m| (m.pool_index, m.template.info.name.clone()))
            .collect()
    }

    /// Starts new episodes in every env; returns the first observations.
    fn reset<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let obs = ObsArrays::zeros(py, self.inner.num_envs(), &self.inner.config);
        let inner = &mut self.inner;
        with_obs(py, &obs, None, |batch, _| inner.reset(batch))?.map_err(value_error)?;
        let dict = PyDict::new(py);
        obs.into_dict("", &dict)?;
        Ok(dict)
    }

    /// Returns `(obs, reward, discount, terminated, truncated, info)`.
    fn step<'py>(
        &mut self,
        py: Python<'py>,
        actions: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyTuple>> {
        let n = self.inner.num_envs();
        let rules = self.inner.rules().clone();
        let numpy = py.import("numpy")?;
        let (dtype, shape): (&str, Vec<usize>) = match rules.mode {
            Mode::Reactive => ("int64", vec![n]),
            Mode::DirectPlan => ("int64", vec![n, rules.direct_plan.horizon]),
            Mode::PathPlanner => ("float32", vec![n, rules.path_planner.waypoints, 2]),
        };
        let array = numpy
            .getattr("ascontiguousarray")?
            .call1((actions, numpy.getattr(dtype)?))?;
        let shape_ok = |s: &[usize]| s == shape.as_slice();
        let obs = ObsArrays::zeros(py, n, &self.inner.config);
        let finals = ObsArrays::zeros(py, n, &self.inner.config);
        let inner = &mut self.inner;
        let infos = if dtype == "int64" {
            let a: PyReadonlyArrayDyn<'py, i64> = array.extract()?;
            if !shape_ok(a.shape()) {
                return Err(value_error(format!("actions must have shape {shape:?}")));
            }
            let slice = a.as_slice()?;
            let actions = if rules.mode == Mode::Reactive {
                Actions::Reactive(slice)
            } else {
                Actions::DirectPlan(slice)
            };
            with_obs(py, &obs, Some(&finals), |o, f| {
                inner.step(actions, o, f.expect("finals"))
            })?
        } else {
            let a: PyReadonlyArrayDyn<'py, f32> = array.extract()?;
            if !shape_ok(a.shape()) {
                return Err(value_error(format!("actions must have shape {shape:?}")));
            }
            let slice = a.as_slice()?;
            with_obs(py, &obs, Some(&finals), |o, f| {
                inner.step(Actions::PathPlanner(slice), o, f.expect("finals"))
            })?
        }
        .map_err(value_error)?;
        let obs_dict = PyDict::new(py);
        obs.into_dict("", &obs_dict)?;
        let info = info_dict(py, &infos)?;
        finals.into_dict("final_", &info)?;
        let column = |f: fn(&StepInfo) -> f32| -> Vec<f32> { infos.iter().map(f).collect() };
        let flags = |f: fn(&StepInfo) -> bool| -> Vec<bool> { infos.iter().map(f).collect() };
        PyTuple::new(
            py,
            [
                obs_dict.into_any(),
                PyArray1::from_vec(py, column(|i| i.reward)).into_any(),
                PyArray1::from_vec(py, column(|i| i.discount)).into_any(),
                PyArray1::from_vec(py, flags(|i| i.terminated)).into_any(),
                PyArray1::from_vec(py, flags(|i| i.truncated)).into_any(),
                info.into_any(),
            ],
        )
    }

    fn set_curriculum(
        &mut self,
        min_distance: f64,
        max_distance: f64,
        max_tortuosity: f64,
    ) -> PyResult<()> {
        self.inner
            .set_curriculum(min_distance, max_distance, max_tortuosity)
            .map_err(value_error)
    }

    /// A* + pure-pursuit actions for the current states, shaped like `step`'s input.
    fn baseline_actions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let n = self.inner.num_envs();
        let rules = self.inner.rules().clone();
        let inner = &self.inner;
        let actions = py.detach(|| inner.baseline_actions());
        Ok(match actions {
            OwnedActions::Discrete(a) => {
                let shape = if rules.mode == Mode::Reactive {
                    vec![n]
                } else {
                    vec![n, rules.direct_plan.horizon]
                };
                PyArray1::from_vec(py, a).reshape(shape)?.into_any()
            }
            OwnedActions::Waypoints(w) => PyArray1::from_vec(py, w)
                .reshape(vec![n, rules.path_planner.waypoints, 2])?
                .into_any(),
        })
    }

    /// Finished-episode records since the last call (only while recording).
    fn episode_records<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let records = self.inner.take_records();
        let json = serde_json::to_string(&records).map_err(|e| value_error(e.to_string()))?;
        from_json(py, &json)
    }

    fn set_recording(&mut self, enabled: bool) {
        self.inner.set_recording(enabled);
    }
}

fn info_dict<'py>(py: Python<'py>, infos: &[StepInfo]) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    let flag = |f: fn(&StepInfo) -> bool| PyArray1::from_vec(py, infos.iter().map(f).collect());
    let float = |f: fn(&StepInfo) -> f32| PyArray1::from_vec(py, infos.iter().map(f).collect());
    let int = |f: fn(&StepInfo) -> i64| PyArray1::from_vec(py, infos.iter().map(f).collect());
    dict.set_item("success", flag(|i| i.success))?;
    dict.set_item("destroyed", flag(|i| i.destroyed))?;
    dict.set_item("timeout", flag(|i| i.timeout))?;
    dict.set_item("stalled", flag(|i| i.stalled))?;
    dict.set_item("path_distance", float(|i| i.path_distance))?;
    dict.set_item("optimal_distance", float(|i| i.optimal_distance))?;
    dict.set_item("optimal_length", float(|i| i.optimal_length))?;
    dict.set_item("ticks", int(|i| i.ticks as i64))?;
    dict.set_item("damage", float(|i| i.damage))?;
    dict.set_item("collisions", int(|i| i.collisions as i64))?;
    for (k, kind) in COLLISION_KINDS.iter().enumerate() {
        let values: Vec<i64> = infos
            .iter()
            .map(|i| i.collisions_by_kind[k] as i64)
            .collect();
        dict.set_item(format!("collisions_{kind}"), PyArray1::from_vec(py, values))?;
    }
    dict.set_item("nest_zone_ticks", int(|i| i.nest_zone_ticks as i64))?;
    dict.set_item("nest_zone_decisions", int(|i| i.nest_zone_decisions as i64))?;
    dict.set_item("reverse_distance", float(|i| i.reverse_distance))?;
    dict.set_item("reversed", flag(|i| i.reversed))?;
    dict.set_item("scenario", int(|i| i.scenario as i64))?;
    dict.set_item("path_length", float(|i| i.path_length))?;
    dict.set_item("speed", float(|i| i.speed))?;
    dict.set_item("target_distance", float(|i| i.target_distance))?;
    dict.set_item("map_index", int(|i| i.map_index as i64))?;
    dict.set_item("vehicle_index", int(|i| i.vehicle_index as i64))?;
    dict.set_item("fuel_index", int(|i| i.fuel_index as i64))?;
    dict.set_item("decisions", int(|i| i.decisions as i64))?;
    dict.set_item("x", float(|i| i.x))?;
    dict.set_item("y", float(|i| i.y))?;
    dict.set_item("orientation", float(|i| i.orientation))?;
    for (k, term) in TERMS.iter().enumerate() {
        let values: Vec<f32> = infos.iter().map(|i| i.terms[k]).collect();
        dict.set_item(format!("reward_{term}"), PyArray1::from_vec(py, values))?;
    }
    Ok(dict)
}

/// Shapes, channel names, vector features, action layout and `spec_hash`.
#[pyfunction]
fn spec(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    let data = Prototypes::from_json(PROTOTYPES).map_err(value_error)?;
    let json =
        serde_json::to_string(&policy_obs::spec(&data)).map_err(|e| value_error(e.to_string()))?;
    from_json(py, &json)
}

#[pymodule]
mod fsim {
    #[pymodule_export]
    use super::{spec, VecEnv};
}
