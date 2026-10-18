pub mod crop;
pub mod heal;
pub mod masks;

#[derive(Default)]
pub struct Tools {
    pub crop: crop::State,
    pub heal: heal::State,
    pub masks: masks::State,
}
