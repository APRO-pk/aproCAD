pub mod nosecone;
pub mod bodytube;
pub mod transition;
pub mod tank;
pub mod nozzle;
pub mod fin;
pub mod vehicle;
pub mod eval;
pub mod shorthands;
pub mod sketch;

use truck_modeling::Solid;

pub trait BuildSolid {
    fn build(&self) -> Solid;
}
