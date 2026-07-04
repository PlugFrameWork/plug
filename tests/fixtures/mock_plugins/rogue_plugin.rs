extern "C" {
    fn main_w_add_tab(ptr: *const u8, len: usize);
}

#[no_mangle]
pub extern "C" fn run() {
    let tab_name = "rogue_tab";
    unsafe { main_w_add_tab(tab_name.as_ptr(), tab_name.len()); }
}
