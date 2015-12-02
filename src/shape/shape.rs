use bbox::BBox;
use diff_geom::DifferentialGeometry;
use ray::Ray;
use transform::transform::ApplyTransform;
use transform::transform::Transform;

use std::sync::atomic::AtomicIsize;

#[derive(Debug, Clone)]
pub struct Shape {
    pub object2world: Transform,
    pub world2object: Transform,
    pub reverse_orientation: bool,
    pub transform_swaps_handedness: bool,
    pub shape_id: isize
}

static NEXT_SHAPE_ID: AtomicIsize = ::std::sync::atomic::ATOMIC_ISIZE_INIT;

impl Shape {
    pub fn new(o2w: Transform, w2o: Transform, ro: bool) -> Shape {
        let swap = o2w.swaps_handedness();
        Shape {
            object2world: o2w,
            world2object: w2o,
            reverse_orientation: ro,
            transform_swaps_handedness: swap,
            shape_id: NEXT_SHAPE_ID.fetch_add(
                1, ::std::sync::atomic::Ordering::Relaxed)
        }
    }
}

impl ::std::cmp::PartialEq for Shape {
    fn eq(&self, other: &Shape) -> bool {
        self.object2world == other.object2world
            && self.world2object == other.world2object
            && self.reverse_orientation == other.reverse_orientation
            && self.transform_swaps_handedness == other.transform_swaps_handedness
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct ShapeIntersection<'a> {
    pub t_hit: f32,
    pub ray_epsilon: f32,
    pub dg: DifferentialGeometry<'a>
}

impl<'a> ShapeIntersection<'a> {
    pub fn new(t: f32, eps: f32, dgeom: DifferentialGeometry<'a>)
           -> ShapeIntersection<'a> {
        ShapeIntersection {
            t_hit: t,
            ray_epsilon: eps,
            dg: dgeom
        }
    }
}

pub trait IsShape {
    fn get_shape<'a>(&'a self) -> &'a Shape;

    fn object_bound(&self) -> BBox;

    fn world_bound(&self) -> BBox {
        let data = self.get_shape();
        data.object2world.xf(self.object_bound())
    }

    // Default is all shapes can intersect..
    fn can_intersect(&self) -> bool { true }

    fn refine<T>(&self) -> Vec<T> where T : IsShape {
        unimplemented!();
    }

    fn intersect(&self, _: &Ray) -> Option<ShapeIntersection> {
        unimplemented!();
    }
    
    fn intersect_p(&self, _: &Ray) -> bool {
        unimplemented!();
    }

    fn get_shading_geometry<'a>(&self, _: &Transform,
                                dg: &DifferentialGeometry<'a>) ->
        DifferentialGeometry<'a> {
            dg.clone()
        }

    fn area(&self) -> f32 { unimplemented!(); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use transform::transform::Transform;

    #[test]
    fn it_can_be_created() {
        let some_shape = Shape::new(Transform::new(), Transform::new(), false);
        assert!(some_shape.shape_id >= 0);
        assert_eq!(Shape::new(Transform::new(), Transform::new(), false),
                   Shape {
                       object2world: Transform::new(),
                       world2object: Transform::new(),
                       reverse_orientation: false,
                       transform_swaps_handedness: false,
                       shape_id: some_shape.shape_id + 1
                   });
    }

    #[test]
    fn two_shapes_can_be_equal() {
        assert_eq!(Shape::new(Transform::new(), Transform::new(), false),
                   Shape::new(Transform::new(), Transform::new(), false));
    }
}