use crate::{Containable, Coordinates};

#[derive(Clone, Copy, PartialEq, Debug, Hash)]
pub struct Circle {
    pub center: Coordinates,
    pub radius: usize,
}

impl Circle {
    pub fn contains<T>(&self, containable: T) -> bool
    where
        T: Containable,
    {
        for coordinates in containable.coordinates() {
            let diff = self.center.abs_diff(coordinates);
            if diff.x.pow(2) + diff.y.pow(2) > self.radius.pow(2) {
                return false;
            }
        }
        true
    }
}
