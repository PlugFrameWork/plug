#[path = ".mod_.rs"]
pub mod terminal;

extern "C" {
    fn get_args(ptr: *mut u8, max_len: usize);
    fn main_w_add_tab(ptr: *const u8, len: usize);
    fn host_set_tab_owner(ptr: *const u8, len: usize);
    fn host_get_tab_label(ptr: *mut u8, max_len: usize) -> i32;
}

static mut MODE: i32 = 0; // 0: Auto, 1: CMD, 2: PS

#[no_mangle]
pub extern "C" fn run() {
    let mut buf = [0u8; 512];
    unsafe { get_args(buf.as_mut_ptr(), buf.len()) };
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    let raw_args = std::str::from_utf8(&buf[..len]).unwrap_or("").trim();

    let args_lower = raw_args.to_lowercase();
    let is_ps_init = args_lower.starts_with("ps");
    let is_cmd_init = args_lower.starts_with("cmd");
    
    if is_ps_init {
        unsafe { MODE = 2 };
    } else if is_cmd_init {
        unsafe { MODE = 1 };
    }

    let mut query = raw_args.to_string();
    if is_ps_init { query = query.replacen("ps", "", 1).trim().to_string(); }
    else if is_cmd_init { query = query.replacen("cmd", "", 1).trim().to_string(); }

    if is_ps_init || is_cmd_init {
        if query.is_empty() {
            let tab_name = if is_ps_init { "PS" } else { "CMD" };
            let plugin_name = "pTerm";
            unsafe { 
                main_w_add_tab(plugin_name.as_ptr(), plugin_name.len());
                host_set_tab_owner(tab_name.as_ptr(), tab_name.len());
            }
            let target = if is_ps_init { "PowerShell" } else { "CMD" };
            terminal::print_info(&format!("{} Terminal Environment opened. Type commands directly.", target));
            
            if is_ps_init { terminal::execute_ps("cd ."); } 
            else { terminal::execute_cmd("cd ."); }
            return;
        }
    }

    let current_mode = unsafe { MODE };
    let final_is_ps = if current_mode == 2 {
        true
    } else if current_mode == 1 {
        false
    } else {
        let mut label_buf = [0u8; 32];
        let label_len = unsafe { host_get_tab_label(label_buf.as_mut_ptr(), label_buf.len()) };
        if label_len > 0 {
            let label = std::str::from_utf8(&label_buf[..label_len as usize]).unwrap_or("");
            label.to_uppercase() == "PS"
        } else {
            false
        }
    };

    if final_is_ps {
        terminal::execute_ps(&query);
    } else {
        terminal::execute_cmd(&query);
    }
}

