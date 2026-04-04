use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::audit::AuditEvent;
use crate::contracts::tool_request::ToolRequest;
use crate::error::KernelError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphNode {
    pub id: u32,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GraphEdge {
    pub id: u32,
    pub from_node_id: u32,
    pub to_node_id: u32,
    pub relation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SectorMemorySnapshot {
    pub sector_id: String,
    pub node_limit: usize,
    pub edge_limit: usize,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub flow_metrics: Vec<FlowMetric>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlowMetric {
    pub relation: String,
    pub from_key: String,
    pub to_key: String,
    pub call_count: u64,
    pub success_count: u64,
    pub failure_count: u64,
    pub total_duration_ms: u128,
    pub max_duration_ms: u128,
    pub critical_delay_count: u64,
    pub last_sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BottleneckEntry {
    pub rank: usize,
    pub relation: String,
    pub from_key: String,
    pub to_key: String,
    pub call_count: u64,
    pub success_count: u64,
    pub failure_count: u64,
    pub total_duration_ms: u128,
    pub avg_duration_ms: u128,
    pub max_duration_ms: u128,
    pub critical_delay_count: u64,
    pub last_sequence: u64,
}

impl SectorMemorySnapshot {
    pub fn bottleneck_report(&self, limit: usize) -> Vec<BottleneckEntry> {
        if limit == 0 {
            return Vec::new();
        }

        let mut metrics = self.flow_metrics.clone();
        metrics.sort_by(|left, right| {
            right
                .critical_delay_count
                .cmp(&left.critical_delay_count)
                .then_with(|| right.total_duration_ms.cmp(&left.total_duration_ms))
                .then_with(|| right.max_duration_ms.cmp(&left.max_duration_ms))
                .then_with(|| right.failure_count.cmp(&left.failure_count))
                .then_with(|| right.call_count.cmp(&left.call_count))
                .then_with(|| left.relation.cmp(&right.relation))
                .then_with(|| left.from_key.cmp(&right.from_key))
                .then_with(|| left.to_key.cmp(&right.to_key))
        });

        metrics
            .into_iter()
            .take(limit)
            .enumerate()
            .map(|(index, metric)| BottleneckEntry {
                rank: index + 1,
                relation: metric.relation,
                from_key: metric.from_key,
                to_key: metric.to_key,
                call_count: metric.call_count,
                success_count: metric.success_count,
                failure_count: metric.failure_count,
                total_duration_ms: metric.total_duration_ms,
                avg_duration_ms: if metric.call_count == 0 {
                    0
                } else {
                    metric.total_duration_ms / u128::from(metric.call_count)
                },
                max_duration_ms: metric.max_duration_ms,
                critical_delay_count: metric.critical_delay_count,
                last_sequence: metric.last_sequence,
            })
            .collect()
    }
}

#[derive(Debug, Default)]
struct SectorMemory {
    node_limit: usize,
    edge_limit: usize,
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
    node_ids_by_key: HashMap<String, u32>,
    edge_ids_by_signature: HashMap<String, u32>,
    flow_metrics_by_signature: HashMap<String, FlowMetric>,
}

impl SectorMemory {
    fn with_limits(node_limit: usize, edge_limit: usize) -> Self {
        Self {
            node_limit,
            edge_limit,
            nodes: Vec::new(),
            edges: Vec::new(),
            node_ids_by_key: HashMap::new(),
            edge_ids_by_signature: HashMap::new(),
            flow_metrics_by_signature: HashMap::new(),
        }
    }

    fn upsert_node(&mut self, sector_id: &str, key: &str) -> Result<u32, KernelError> {
        if let Some(node_id) = self.node_ids_by_key.get(key) {
            return Ok(*node_id);
        }

        if self.nodes.len() >= self.node_limit {
            return Err(KernelError::MemoryNodeLimitReached {
                sector_id: sector_id.into(),
                limit: self.node_limit,
            });
        }

        let node_id = self.nodes.len() as u32;
        let node = GraphNode {
            id: node_id,
            key: key.into(),
        };

        self.nodes.push(node);
        self.node_ids_by_key.insert(key.into(), node_id);
        Ok(node_id)
    }

    fn add_edge(
        &mut self,
        sector_id: &str,
        from_key: &str,
        to_key: &str,
        relation: &str,
    ) -> Result<u32, KernelError> {
        let from_node_id = self.upsert_node(sector_id, from_key)?;
        let to_node_id = self.upsert_node(sector_id, to_key)?;
        let signature = format!("{from_node_id}:{to_node_id}:{relation}");

        if let Some(edge_id) = self.edge_ids_by_signature.get(&signature) {
            return Ok(*edge_id);
        }

        if self.edges.len() >= self.edge_limit {
            return Err(KernelError::MemoryEdgeLimitReached {
                sector_id: sector_id.into(),
                limit: self.edge_limit,
            });
        }
        let edge_id = self.edges.len() as u32;

        self.edges.push(GraphEdge {
            id: edge_id,
            from_node_id,
            to_node_id,
            relation: relation.into(),
        });
        self.edge_ids_by_signature.insert(signature, edge_id);

        Ok(edge_id)
    }

    fn snapshot(&self, sector_id: &str) -> SectorMemorySnapshot {
        let mut flow_metrics = self
            .flow_metrics_by_signature
            .values()
            .cloned()
            .collect::<Vec<_>>();
        flow_metrics.sort_by(|left, right| {
            (&left.from_key, &left.relation, &left.to_key).cmp(&(
                &right.from_key,
                &right.relation,
                &right.to_key,
            ))
        });

        SectorMemorySnapshot {
            sector_id: sector_id.into(),
            node_limit: self.node_limit,
            edge_limit: self.edge_limit,
            nodes: self.nodes.clone(),
            edges: self.edges.clone(),
            flow_metrics,
        }
    }

    fn record_flow_metric(
        &mut self,
        sector_id: &str,
        from_key: &str,
        to_key: &str,
        relation: &str,
        duration_ms: u128,
        success: bool,
        critical_delay_ms: u64,
        sequence: u64,
    ) -> Result<(), KernelError> {
        self.add_edge(sector_id, from_key, to_key, relation)?;

        let signature = format!("{from_key}:{relation}:{to_key}");
        let metric = self
            .flow_metrics_by_signature
            .entry(signature)
            .or_insert_with(|| FlowMetric {
                relation: relation.into(),
                from_key: from_key.into(),
                to_key: to_key.into(),
                call_count: 0,
                success_count: 0,
                failure_count: 0,
                total_duration_ms: 0,
                max_duration_ms: 0,
                critical_delay_count: 0,
                last_sequence: sequence,
            });

        metric.call_count += 1;
        metric.total_duration_ms += duration_ms;
        metric.max_duration_ms = metric.max_duration_ms.max(duration_ms);
        metric.last_sequence = sequence;

        if success {
            metric.success_count += 1;
        } else {
            metric.failure_count += 1;
        }

        if duration_ms >= u128::from(critical_delay_ms) {
            metric.critical_delay_count += 1;
        }

        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct GraphMemory {
    sectors: Mutex<HashMap<String, SectorMemory>>,
}

impl GraphMemory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ensure_sector(&self, sector_id: &str, node_limit: usize, edge_limit: usize) {
        let mut sectors = self.sectors.lock().expect("graph memory mutex poisoned");
        sectors
            .entry(sector_id.into())
            .or_insert_with(|| SectorMemory::with_limits(node_limit, edge_limit));
    }

    pub fn upsert_node(
        &self,
        sector_id: &str,
        key: &str,
        node_limit: usize,
        edge_limit: usize,
    ) -> Result<u32, KernelError> {
        let mut sectors = self.sectors.lock().expect("graph memory mutex poisoned");
        let sector = sectors
            .entry(sector_id.into())
            .or_insert_with(|| SectorMemory::with_limits(node_limit, edge_limit));
        sector.upsert_node(sector_id, key)
    }

    pub fn add_edge(
        &self,
        sector_id: &str,
        from_key: &str,
        to_key: &str,
        relation: &str,
        node_limit: usize,
        edge_limit: usize,
    ) -> Result<u32, KernelError> {
        let mut sectors = self.sectors.lock().expect("graph memory mutex poisoned");
        let sector = sectors
            .entry(sector_id.into())
            .or_insert_with(|| SectorMemory::with_limits(node_limit, edge_limit));
        sector.add_edge(sector_id, from_key, to_key, relation)
    }

    pub fn sector_snapshot(&self, sector_id: &str) -> Result<SectorMemorySnapshot, KernelError> {
        let sectors = self.sectors.lock().expect("graph memory mutex poisoned");
        let sector = sectors
            .get(sector_id)
            .ok_or_else(|| KernelError::UnknownMemorySector {
                sector_id: sector_id.into(),
            })?;
        Ok(sector.snapshot(sector_id))
    }

    pub fn record_flow_metric(
        &self,
        sector_id: &str,
        from_key: &str,
        to_key: &str,
        relation: &str,
        duration_ms: u128,
        success: bool,
        critical_delay_ms: u64,
        sequence: u64,
        node_limit: usize,
        edge_limit: usize,
    ) -> Result<(), KernelError> {
        let mut sectors = self.sectors.lock().expect("graph memory mutex poisoned");
        let sector = sectors
            .entry(sector_id.into())
            .or_insert_with(|| SectorMemory::with_limits(node_limit, edge_limit));
        sector.record_flow_metric(
            sector_id,
            from_key,
            to_key,
            relation,
            duration_ms,
            success,
            critical_delay_ms,
            sequence,
        )
    }
}

#[derive(Debug)]
pub struct KernelMemory {
    pub kernel_sector: String,
    pub node_limit: usize,
    pub edge_limit: usize,
    graph: GraphMemory,
}

impl KernelMemory {
    pub fn new(kernel_sector: String, node_limit: usize, edge_limit: usize) -> Self {
        let graph = GraphMemory::new();
        graph.ensure_sector(&kernel_sector, node_limit, edge_limit);

        Self {
            kernel_sector,
            node_limit,
            edge_limit,
            graph,
        }
    }

    pub fn upsert_node(&self, sector_id: &str, key: &str) -> Result<u32, KernelError> {
        self.ensure_kernel_sector(sector_id)?;
        self.graph
            .upsert_node(sector_id, key, self.node_limit, self.edge_limit)
    }

    pub fn add_edge(
        &self,
        sector_id: &str,
        from_key: &str,
        to_key: &str,
        relation: &str,
    ) -> Result<u32, KernelError> {
        self.ensure_kernel_sector(sector_id)?;
        self.graph.add_edge(
            sector_id,
            from_key,
            to_key,
            relation,
            self.node_limit,
            self.edge_limit,
        )
    }

    pub fn sector_snapshot(&self, sector_id: &str) -> Result<SectorMemorySnapshot, KernelError> {
        self.ensure_kernel_sector(sector_id)?;
        self.graph.sector_snapshot(sector_id)
    }

    pub fn bottleneck_report(
        &self,
        sector_id: &str,
        limit: usize,
    ) -> Result<Vec<BottleneckEntry>, KernelError> {
        let snapshot = self.sector_snapshot(sector_id)?;
        Ok(snapshot.bottleneck_report(limit))
    }

    pub fn record_tool_activity(
        &self,
        request: &ToolRequest,
        audit_event: &AuditEvent,
        critical_delay_ms: u64,
    ) -> Result<(), KernelError> {
        let sector_id = self.kernel_sector.as_str();
        let kernel_node = format!("kernel:{}", self.kernel_sector);
        let request_node = format!("request:{}", request.request_id);
        let tool_node = format!("tool:{}", request.tool_name);
        let audit_node = format!("audit:{}:{}", audit_event.request_id, audit_event.sequence);
        let status_node = if audit_event.success {
            "status:success".to_string()
        } else {
            "status:failure".to_string()
        };

        self.upsert_node(sector_id, &kernel_node)?;
        self.upsert_node(sector_id, &request_node)?;
        self.upsert_node(sector_id, &tool_node)?;
        self.upsert_node(sector_id, &audit_node)?;
        self.upsert_node(sector_id, &status_node)?;

        self.add_edge(sector_id, &kernel_node, &request_node, "owns_request")?;
        self.add_edge(sector_id, &request_node, &tool_node, "targets_tool")?;
        self.add_edge(sector_id, &request_node, &audit_node, "emits_audit")?;
        self.add_edge(sector_id, &audit_node, &status_node, "records_status")?;
        self.graph.record_flow_metric(
            sector_id,
            &kernel_node,
            &tool_node,
            "dispatches_tool",
            audit_event.duration_ms,
            audit_event.success,
            critical_delay_ms,
            audit_event.sequence,
            self.node_limit,
            self.edge_limit,
        )?;
        self.graph.record_flow_metric(
            sector_id,
            &tool_node,
            &status_node,
            "returns_status",
            audit_event.duration_ms,
            audit_event.success,
            critical_delay_ms,
            audit_event.sequence,
            self.node_limit,
            self.edge_limit,
        )?;

        Ok(())
    }

    fn ensure_kernel_sector(&self, sector_id: &str) -> Result<(), KernelError> {
        if sector_id == self.kernel_sector {
            return Ok(());
        }

        Err(KernelError::MemorySectorAccessDenied {
            kernel_sector: self.kernel_sector.clone(),
            requested_sector: sector_id.into(),
        })
    }
}
