#[derive(Debug, Clone)]
pub struct ToolDescriptor {
    pub name: &'static str,
    pub side_effects: bool,
}

pub fn builtin_tools() -> Vec<ToolDescriptor> {
    vec![
        ToolDescriptor {
            name: "read_file",
            side_effects: false,
        },
        ToolDescriptor {
            name: "list_dir",
            side_effects: false,
        },
        ToolDescriptor {
            name: "file_exists",
            side_effects: false,
        },
    ]
}
