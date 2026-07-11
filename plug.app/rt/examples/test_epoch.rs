use wasmer::{Store, Cranelift};

fn main() {
    let mut config = Cranelift::default();
    
    // Check available methods - try to find epoch/fuel
    let config_debug = format!("{:?}", config);
    println!("Config: {}", config_debug);
    
    let store = Store::new(config);
    println!("Store created: {:?}", store);
    
    // Check if Store has add_fuel
    println!("Store methods: add_fuel={:?}", 
        std::any::type_name_of_val(&store.add_fuel));
}
