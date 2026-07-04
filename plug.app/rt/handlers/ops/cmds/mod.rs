use std::collections::HashMap;
use std::sync::Mutex;
use once_cell::sync::Lazy;

pub mod c_abt;
pub mod c_e;
pub mod c_q;
pub mod c_nt;
pub mod c_plug;

pub static SESSION_PLUGINS: Lazy<Mutex<HashMap<String, (String, String)>>> = Lazy::new(|| Mutex::new(HashMap::new()));

