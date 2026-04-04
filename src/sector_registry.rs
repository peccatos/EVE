use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectorDescriptor {
    pub sector_id: String,
    pub enabled: bool,
    pub accepts_handoffs: bool,
    pub supported_tools: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectorRegistry {
    sectors: HashMap<String, SectorDescriptor>,
}

impl SectorRegistry {
    pub fn new(sectors: Vec<SectorDescriptor>) -> Self {
        let sectors = sectors
            .into_iter()
            .map(|descriptor| (descriptor.sector_id.clone(), descriptor))
            .collect::<HashMap<_, _>>();

        Self { sectors }
    }

    pub fn default_for_builtin_tools(source_sector: &str) -> Self {
        Self::new(vec![
            SectorDescriptor {
                sector_id: source_sector.into(),
                enabled: true,
                accepts_handoffs: false,
                supported_tools: vec!["read_file".into(), "list_dir".into(), "file_exists".into()],
            },
            SectorDescriptor {
                sector_id: "edge".into(),
                enabled: true,
                accepts_handoffs: true,
                supported_tools: vec!["list_dir".into(), "file_exists".into()],
            },
        ])
    }

    pub fn get(&self, sector_id: &str) -> Option<&SectorDescriptor> {
        self.sectors.get(sector_id)
    }

    pub fn supports_tool(&self, sector_id: &str, tool_name: &str) -> bool {
        self.get(sector_id)
            .map(|sector| sector.supported_tools.iter().any(|tool| tool == tool_name))
            .unwrap_or(false)
    }
}
