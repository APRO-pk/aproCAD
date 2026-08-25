use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Material {
    Named(String),
    Defined(MaterialDef),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MaterialDef {
    pub name: String,
    pub density: f64,
    pub modulus: f64,
    pub yield_stress: f64,
    pub density_unit: DensityUnit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DensityUnit {
    KgM3,
    GCm3,
}

pub fn density_for_name(name: &str) -> f64 {
    match name {
        "Al-6061-T6" => 2700.0,     // kg/m³
        "Al-7075-T6" => 2810.0,
        "Steel-4130" => 7830.0,
        "Steel-304" => 8000.0,
        "Ti-6Al-4V" => 4430.0,
        "Inconel-718" => 8190.0,
        "Mg-AZ31B" => 1780.0,
        "CFRP" => 1600.0,
        "G10-FR4" => 1800.0,
        "Plywood" => 600.0,
        "ABS" => 1040.0,
        "PLA" => 1240.0,
        _ => 1000.0,  // default density
    }
}

pub fn density_kg_per_mm3(name: &str) -> f64 {
    density_for_name(name) / 1.0e9
}
