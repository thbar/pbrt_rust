use bbox::BBox;
use bbox::Union;
use geometry::point::Point;
use geometry::vector::Vector;
use quaternion::Quaternion;
use ray::Ray;
use ray::RayDifferential;
use transform::matrix4x4::Matrix4x4;
use transform::transform::ApplyTransform;
use transform::transform::Transform;
use utils::Lerp;

macro_rules! check_mat {
    ($m1: expr, $m2: expr) => {{
        let x = ($m1).clone();
        let y = ($m2).clone();
        for i in 0..4 {
            for j in 0..4 {
                let diff = (x[i][j] - y[i][j]).abs();
                if diff >= 5e-5 {
                    println!("m1: {:?}", x);
                    println!("m2: {:?}", y);
                    println!("Matrices differ at {:?} by {:?}", (i, j), diff);
                    panic!();
                }
            }
        }
    }}
}

macro_rules! check_animated_xform {
    ($q1: expr, $q2: expr) => {{
        let xf1 = &(($q1).clone());
        let xf2 = &(($q2).clone());

        if (xf1.start_time - xf2.start_time).abs() >= 1e-6 {
            println!("Animated transforms differ on start_time");
            println!("xform1: {:?}", xf1.start_time);
            println!("xform2: {:?}", xf2.start_time);
            panic!();
        }

        if (xf1.end_time - xf2.end_time).abs() >= 1e-6 {
            println!("Animated transforms differ on end_time");
            println!("xform1: {:?}", xf1.end_time);
            println!("xform2: {:?}", xf2.end_time);
            panic!();
        }

        if xf1.actually_animated != xf2.actually_animated {
            println!("Animated transforms differ on actually_animated");
            println!("xform1: {:?}", xf1.actually_animated);
            println!("xform2: {:?}", xf2.actually_animated);
            panic!();
        }

        if (xf1.t1.clone() - xf2.t1.clone()).length_squared() >= 1e-6 {
            println!("Animated transforms differ on t1");
            println!("xform1: {:?}", xf1.t1);
            println!("xform2: {:?}", xf2.t1);
            panic!();
        }

        if (xf1.t2.clone() - xf2.t2.clone()).length_squared() >= 1e-6 {
            println!("Animated transforms differ on t2");
            println!("xform1: {:?}", xf1.t2);
            println!("xform2: {:?}", xf2.t2);
            panic!();
        }

        if (xf1.r1.dot(&xf2.r1).powi(2) - 1.0).abs() >= 1e-6 {
            println!("Animated transforms differ on r1");
            println!("xform1: {:?}", xf1.r1);
            println!("xform2: {:?}", xf2.r1);
            panic!();
        }

        if (xf1.r2.dot(&xf2.r2).powi(2) - 1.0).abs() >= 1e-6 {
            println!("Animated transforms differ on r2");
            println!("xform1: {:?}", xf1.r2);
            println!("xform2: {:?}", xf2.r2);
            panic!();
        }

        check_mat!(xf1.s1, xf2.s1);
        check_mat!(xf1.s2, xf2.s2);
    }}
}

#[derive(Debug, PartialEq, Clone)]
pub struct AnimatedTransform {
    start_time: f32,
    end_time: f32,
    start_transform: Transform,
    end_transform: Transform,
    actually_animated: bool,
    t1: Vector, t2: Vector, t_animated: bool,
    r1: Quaternion, r2: Quaternion, r_animated: bool,
    s1: Matrix4x4, s2: Matrix4x4, s_animated: bool
}

impl AnimatedTransform {
    pub fn new(transform1: Transform, time1: f32,
               transform2: Transform, time2: f32) -> AnimatedTransform {
        let (t1, r1, s1) = AnimatedTransform::decompose(&transform1);
        let (t2, r2, s2) = AnimatedTransform::decompose(&transform2);
        let animated = transform1.ne(&transform2);
        let t_anim = (&t1 - &t2).length_squared() >= 1e-6;
        let r_anim = {
            let dx = (r1.v.x - r2.v.x).abs();
            let dy = (r1.v.y - r2.v.y).abs();
            let dz = (r1.v.z - r2.v.z).abs();
            let dw = (r1.w - r2.w).abs();
            (dx * dx + dy * dy + dz * dz + dw * dw) >= 1e-6
        };

        let s_anim = {
            s1.m.iter().zip(s2.m.iter()).fold(0.0, |acc, (r1, r2)| {
                let d0 = (r1[0] - r2[0]).abs();
                let d1 = (r1[1] - r2[1]).abs();
                let d2 = (r1[2] - r2[2]).abs();
                let d3 = (r1[3] - r2[3]).abs();
                acc + d0 * d0 + d1 * d1 + d2 * d2 + d3 * d3
            }) >= 1e-6
        };

        AnimatedTransform {
            start_time: time1,
            end_time: time2,
            start_transform: transform1,
            end_transform: transform2,
            actually_animated: animated,
            t1: t1, t2: t2, t_animated: t_anim,
            r1: r1, r2: r2, r_animated: r_anim,
            s1: s1, s2: s2, s_animated: s_anim
        }
    }

    pub fn identity() -> AnimatedTransform {
        AnimatedTransform::new(Transform::new(), 0.0, Transform::new(), 1.0)
    }

    pub fn interpolate(&self, time: f32) -> Transform {
        // Handle boundary conditions for matrix interpolation
        if !self.actually_animated || time <= self.start_time {
            return self.start_transform.clone();
        }

        if time >= self.end_time {
            return self.end_transform.clone();
        }

        let dt = (time - self.start_time) / (self.end_time - self.start_time);

        // Interpolate translation at dt
        let trans = if self.t_animated {
            self.t1.lerp(&self.t2, dt)
        } else {
            self.t1.clone()
        };

        // Interpolate rotation at dt
        let rotate = if self.r_animated {
            self.r1.lerp(&self.r2, dt)
        } else {
            self.r1.clone()
        };

        // Interpolate scale at dt
        let scale = if self.s_animated {
            self.s1.lerp(&self.s2, dt)
        } else {
            self.s1.clone()
        };

        // Compute interpolated matrix as product of interpolated components
        Transform::translate(&trans) *
            Transform::from(rotate) *
            Transform::from(scale)
    }

    fn decompose(transform: &Transform) -> (Vector, Quaternion, Matrix4x4) {
        let tm = transform.get_matrix();

        // Extract translation T from the transformation matrix
        let t = Vector::new_with(tm[0][3], tm[1][3], tm[2][3]);

        // Compute new transformation matrix M without translation
        let mut m = tm.clone();
        {
            // Scope the borrow of m...
            let m_ref = &mut m;
            for i in 0..3 {
                m_ref[i][3] = 0f32;
            }
            m_ref[3] = [0f32, 0f32, 0f32, 1f32];
        };

        // Extract rotation R from transformation matrix
        let mut r = m.clone();
        for _ in 0..100 {
            // Compute next matrix r_next in series
            let r_next = 0.5 * (&r + r.clone().invert().transpose());

            // Compute norm of difference between r and r_next
            let norm = (0..3).fold(0f32, |acc, i| {
                let r_ref = &r;
                let r_next_ref = &r_next;
                acc.max((r_ref[i][0] - r_next_ref[i][0]).abs() +
                        (r_ref[i][1] - r_next_ref[i][1]).abs() +
                        (r_ref[i][2] - r_next_ref[i][2]).abs())
            });

            if norm < 0.0001f32 {
                break;
            }

            r = r_next;
        }

        // Compute scale S using rotation and original matrix
        let s = r.inverse() * m;
        (t, Quaternion::from(r), s)
    }

    pub fn motion_bounds(&self, b: &BBox, use_inverse: bool) -> BBox {
        if !self.actually_animated {
            return if use_inverse {
                self.start_transform.inverse().t(b)
            } else {
                self.start_transform.t(b)
            };
        }

        let num_steps = 128;
        (0..num_steps).fold(BBox::new(), |bbox, i| {
            let t = self.start_time.lerp(&self.end_time,
                                         ((i as f32) / ((num_steps - 1) as f32)));
            bbox.unioned_with(
                if use_inverse {
                    self.interpolate(t).invert().t(b)
                } else {
                    self.interpolate(t).t(b)
                })
        })
    }

    pub fn xfpt(&self, time: f32, p: Point) -> Point {
        self.interpolate(time).xf(p)
    }

    pub fn tpt(&self, time: f32, p: &Point) -> Point {
        self.xfpt(time, p.clone())
    }

    pub fn xfvec(&self, time: f32, v: Vector) -> Vector {
        self.interpolate(time).xf(v)
    }

    pub fn tvec(&self, time: f32, v: &Vector) -> Vector {
        self.xfvec(time, v.clone())
    }
}

impl ApplyTransform<Ray> for AnimatedTransform {
    fn xf(&self, r: Ray) -> Ray {
        let mut ret = r.clone();
        let t = f32::from(r.time);
        ret.o = self.tpt(t, &ret.o);
        ret.d = self.tvec(t, &ret.d);
        ret
    }
}

impl ApplyTransform<RayDifferential> for AnimatedTransform {
    fn xf(&self, r: RayDifferential) -> RayDifferential {
        let mut ret = r.clone();
        let t = f32::from(r.ray.time);
        ret.ray.o = self.tpt(t, &ret.ray.o);
        ret.ray.d = self.tvec(t, &ret.ray.d);
        ret
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bbox::BBox;
    use geometry::point::Point;
    use geometry::vector::Dot;
    use geometry::vector::Vector;
    use quaternion::Quaternion;
    use ray::Ray;
    use ray::RayDifferential;
    use transform::matrix4x4::Matrix4x4;
    use transform::transform::Transform;
    use transform::transform::ApplyTransform;

    #[test]
    fn it_can_be_created() {
        let from = Transform::new();
        let mut expected_anim = AnimatedTransform {
            start_time: 0.0,
            end_time: 0.0,
            start_transform: from.clone(),
            end_transform: from.clone(),
            actually_animated: false,
            t1: Vector::new(), t2: Vector::new(), t_animated: false,
            r1: Quaternion::new(), r2: Quaternion::new(), r_animated: false,
            s1: Matrix4x4::new(), s2: Matrix4x4::new(), s_animated: false
        };

        assert_eq!(expected_anim,
                   AnimatedTransform::new(from.clone(), 0.0, from.clone(), 0.0));

         expected_anim = AnimatedTransform {
            start_time: 0.0,
            end_time: 1.0,
            start_transform: from.clone(),
            end_transform: from.clone(),
            actually_animated: false,
            t1: Vector::new(), t2: Vector::new(), t_animated: false,
            r1: Quaternion::new(), r2: Quaternion::new(), r_animated: false,
            s1: Matrix4x4::new(), s2: Matrix4x4::new(), s_animated: false,
        };

        assert_eq!(expected_anim,
                   AnimatedTransform::new(from.clone(), 0.0, from.clone(), 1.0));

        let to = Transform::translate(&Vector::new_with(1.0, 1.0, 1.0)) *
            Transform::rotate_y(45.0);

         expected_anim = AnimatedTransform {
             start_time: 0.0,
             end_time: 1.0,
             start_transform: from.clone(),
             end_transform: to.clone(),
             actually_animated: true,
             t1: Vector::new(), t2: Vector::new_with(1.0, 1.0, 1.0), t_animated: true,
             r1: Quaternion::new(),
             r2: Quaternion::new_with(0.0, 0.38268343236, 0.0, 0.92387953251),
             r_animated: true,
             s1: Matrix4x4::new(), s2: Matrix4x4::new(), s_animated: false
        };

        check_animated_xform!(
            expected_anim,
            AnimatedTransform::new(from.clone(), 0.0, to.clone(), 1.0));
    }

    #[test]
    fn it_can_bound_motion() {
        let to = Transform::translate(&Vector::new_with(1.0, 1.0, 1.0)) *
            Transform::rotate_y(45.0);
        let anim_xform = AnimatedTransform::new(
            Transform::new(), 1.0, to.clone(), 2.0);
        let simple_box = BBox::new_with(Point::new_with(-1.0, -1.0, -1.0),
                                        Point::new_with(1.0, 1.0, 1.0));
        assert_eq!(anim_xform.motion_bounds(&simple_box, false),
                   BBox::new_with(Point::new_with(-1.0, -1.0, -1.0),
                                  Point::new_with(1.0 + 2f32.sqrt(),
                                                  2.0,
                                                  1.0 + 2f32.sqrt())));

        assert_eq!(anim_xform.motion_bounds(&simple_box, true),
                   // !KLUDGE! This x-value *looks* right but may not *be* right...
                   // Basically I have little intuitive sense for what happens to the
                   // box if you translate before you rotate...
                   BBox::new_with(Point::new_with(-1.6106474, -2.0,
                                                  -2.0*2f32.sqrt()),
                                  Point::new_with(2f32.sqrt(), 1.0, 1.0)));

        assert_eq!(AnimatedTransform::new(
            Transform::new(), 0.0, Transform::new(), 1.0).
                   motion_bounds(&simple_box, false), simple_box);

        assert_eq!(AnimatedTransform::new(
            Transform::new(), 0.0, to, 0.0).
                   motion_bounds(&simple_box, false), simple_box);
    }

    #[test]
    fn it_can_transform_points() {
        let from = Transform::translate(&Vector::new_with(1.0, 2.0, 3.0));
        let to = Transform::translate(&Vector::new_with(-3.0, 0.0, -14.0));
        let mut xform = AnimatedTransform::new(from, 0.0, to, 1.0);
        assert_eq!(xform.xfpt(0.0, Point::new()), Point::new_with(1.0, 2.0, 3.0));
        assert_eq!(xform.xfpt(1.0, Point::new()), Point::new_with(-3.0, 0.0, -14.0));
        assert_eq!(xform.xfpt(0.5, Point::new()), Point::new_with(-1.0, 1.0, -5.5));

        assert_eq!(xform.tpt(0.0, &Point::new()), Point::new_with(1.0, 2.0, 3.0));
        assert_eq!(xform.tpt(1.0, &Point::new()), Point::new_with(-3.0, 0.0, -14.0));
        assert_eq!(xform.tpt(0.5, &Point::new()), Point::new_with(-1.0, 1.0, -5.5));

        let from2 = Transform::new();
        let to2 = Transform::translate(&Vector::new_with(1.0, 1.0, 1.0)) *
            Transform::scale(2.0, 1.5, 0.5) *
            Transform::rotate_x(45.0);
        xform = AnimatedTransform::new(from2, 0.0, to2, 10.0);
        let pt = Point::new_with(1.0, 1.0, 1.0);
        let expected = Point::new_with(3.0, 1.0, 1.0 + 0.5*2f32.sqrt());

        assert_eq!(xform.xfpt(0.0, pt.clone()), pt);
        assert_eq!(xform.xfpt(10.0, pt.clone()), expected);

        // !KLUDGE! These numbers look right -- but they might not *be* right....
        assert_eq!(xform.xfpt(5.0, pt.clone()),
                   Point::new_with(2.0, 0.90589696, 1.4799222));

        let xfpt = xform.xfpt(9.99999, pt.clone());
        assert!((xfpt.x - expected.x).abs() < 1e-5);
        assert!((xfpt.y - expected.y).abs() < 1e-5);
        assert!((xfpt.z - expected.z).abs() < 1e-5);
    }

    #[test]
    fn it_can_transform_vectors() {
        let from = Transform::translate(&Vector::new_with(1.0, 2.0, 3.0));
        let to = Transform::translate(&Vector::new_with(-3.0, 0.0, -14.0));
        let mut xform = AnimatedTransform::new(from, 0.0, to, 1.0);

        // No matter what, just translated transforms shouldn't change
        // vectors...
        let random_vector = Vector::new_with(1.0, -10.3, 16.2);
        assert_eq!(xform.xfvec(0.0, random_vector.clone()), random_vector);
        assert_eq!(xform.xfvec(1.0, random_vector.clone()), random_vector);
        assert_eq!(xform.xfvec(0.5, random_vector.clone()), random_vector);

        assert_eq!(xform.tvec(0.0, &random_vector), random_vector);
        assert_eq!(xform.tvec(1.0, &random_vector), random_vector);
        assert_eq!(xform.tvec(0.5, &random_vector), random_vector);

        let from2 = Transform::new();
        let to2 = Transform::translate(&Vector::new_with(1.0, 1.0, 1.0)) *
            Transform::scale(2.0, 1.5, 0.5) *
            Transform::rotate_x(45.0);
        xform = AnimatedTransform::new(from2, 0.0, to2, 10.0);
        let v = Vector::new_with(1.0, 1.0, 1.0);
        let expected = Vector::new_with(2.0, 0.0, 0.5*2f32.sqrt());

        assert_eq!(xform.xfvec(0.0, v.clone()), v);
        assert_eq!(xform.xfvec(10.0, v.clone()), expected);

        // !KLUDGE! These numbers look right -- but they might not *be* right....
        assert_eq!(xform.xfvec(5.0, v.clone()),
                   Vector::new_with(1.5, 0.40589696, 0.9799222));

        let xfvec = xform.xfvec(9.99999, v.clone());
        assert!((xfvec.x - expected.x).abs() < 1e-5);
        assert!((xfvec.y - expected.y).abs() < 1e-5);
        assert!((xfvec.z - expected.z).abs() < 1e-5);
    }

    #[test]
    fn it_can_transform_rays() {
        let from = Transform::translate(&Vector::new_with(1.0, 2.0, 3.0));
        let to = Transform::translate(&Vector::new_with(-3.0, 0.0, -14.0));
        let mut xform = AnimatedTransform::new(from, 0.0, to, 1.0);

        let mut p = Point::new();
        let mut v = Vector::new_with(1.0, -10.3, 16.2);
        let mut r = Ray::new_with(p.clone(), v.clone(), 0.0);
        let mut rd = RayDifferential::new_with(p.clone(), v.clone(), 0.0);

        r.set_time(0.0);
        assert_eq!(xform.xf(r.clone()).o, Point::new_with(1.0, 2.0, 3.0));
        assert_eq!(xform.xf(r.clone()).d, v);

        rd.ray.set_time(0.0);
        assert_eq!(xform.xf(rd.clone()).ray.o, Point::new_with(1.0, 2.0, 3.0));
        assert_eq!(xform.xf(rd.clone()).ray.d, v);

        r.set_time(1.0);
        assert_eq!(xform.xf(r.clone()).o, Point::new_with(-3.0, 0.0, -14.0));
        assert_eq!(xform.xf(r.clone()).d, v);

        rd.ray.set_time(1.0);
        assert_eq!(xform.xf(rd.clone()).ray.o, Point::new_with(-3.0, 0.0, -14.0));
        assert_eq!(xform.xf(rd.clone()).ray.d, v);

        r.set_time(0.5);
        assert_eq!(xform.xf(r.clone()).o, Point::new_with(-1.0, 1.0, -5.5));
        assert_eq!(xform.xf(r.clone()).d, v);

        rd.ray.set_time(0.5);
        assert_eq!(xform.xf(rd.clone()).ray.o, Point::new_with(-1.0, 1.0, -5.5));
        assert_eq!(xform.xf(rd.clone()).ray.d, v);

        r.set_time(0.0);
        assert_eq!(xform.t(&r).o, Point::new_with(1.0, 2.0, 3.0));
        assert_eq!(xform.t(&r).d, v);

        rd.ray.set_time(0.0);
        assert_eq!(xform.t(&rd).ray.o, Point::new_with(1.0, 2.0, 3.0));
        assert_eq!(xform.t(&rd).ray.d, v);

        r.set_time(1.0);
        assert_eq!(xform.t(&r).o, Point::new_with(-3.0, 0.0, -14.0));
        assert_eq!(xform.t(&r).d, v);

        rd.ray.set_time(1.0);
        assert_eq!(xform.t(&rd).ray.o, Point::new_with(-3.0, 0.0, -14.0));
        assert_eq!(xform.t(&rd).ray.d, v);

        r.set_time(0.5);
        assert_eq!(xform.t(&r).o, Point::new_with(-1.0, 1.0, -5.5));
        assert_eq!(xform.t(&r).d, v);

        rd.ray.set_time(0.5);
        assert_eq!(xform.t(&rd).ray.o, Point::new_with(-1.0, 1.0, -5.5));
        assert_eq!(xform.t(&rd).ray.d, v);

        let from2 = Transform::new();
        let to2 = Transform::translate(&Vector::new_with(1.0, 1.0, 1.0)) *
            Transform::scale(2.0, 1.5, 0.5) *
            Transform::rotate_x(45.0);
        xform = AnimatedTransform::new(from2, 0.0, to2, 10.0);
        p = Point::new_with(1.0, 1.0, 1.0);
        v = Vector::new_with(1.0, 1.0, 1.0);
        r = Ray::new_with(p.clone(), v.clone(), 0.0);
        rd = RayDifferential::new_with(p.clone(), v.clone(), 0.0);

        let o_expected = Point::new_with(3.0, 1.0, 1.0 + 0.5*2f32.sqrt());
        let d_expected = Vector::new_with(2.0, 0.0, 0.5*2f32.sqrt());

        r.set_time(0.0);
        assert_eq!(xform.xf(r.clone()).o, p);
        assert_eq!(xform.xf(r.clone()).d, v);

        r.set_time(10.0);
        assert_eq!(xform.xf(r.clone()).o, o_expected);
        assert_eq!(xform.xf(r.clone()).d, d_expected);

        rd.ray.set_time(0.0);
        assert_eq!(xform.xf(rd.clone()).ray.o, p);
        assert_eq!(xform.xf(rd.clone()).ray.d, v);

        rd.ray.set_time(10.0);
        assert_eq!(xform.xf(rd.clone()).ray.o, o_expected);
        assert_eq!(xform.xf(rd.clone()).ray.d, d_expected);

        // !KLUDGE! These numbers look right -- but they might not *be* right....
        r.set_time(5.0);
        rd.ray.set_time(5.0);
        assert_eq!(xform.xf(r.clone()).o, Point::new_with(2.0, 0.90589696, 1.4799222));
        assert_eq!(xform.xf(r.clone()).d, Vector::new_with(1.5, 0.40589696, 0.9799222));
        assert_eq!(xform.xf(rd.clone()).ray.o, Point::new_with(2.0, 0.90589696, 1.4799222));
        assert_eq!(xform.xf(rd.clone()).ray.d, Vector::new_with(1.5, 0.40589696, 0.9799222));

        r.set_time(9.99999);
        rd.ray.set_time(9.99999);

        let xfr = xform.xf(r.clone());
        assert!((xfr.o.x - o_expected.x).abs() < 1e-5);
        assert!((xfr.o.y - o_expected.y).abs() < 1e-5);
        assert!((xfr.o.z - o_expected.z).abs() < 1e-5);

        assert!((xfr.d.x - d_expected.x).abs() < 1e-5);
        assert!((xfr.d.y - d_expected.y).abs() < 1e-5);
        assert!((xfr.d.z - d_expected.z).abs() < 1e-5);

        let xfrd = xform.xf(rd.clone());
        assert!((xfrd.ray.d.x - d_expected.x).abs() < 1e-5);
        assert!((xfrd.ray.d.y - d_expected.y).abs() < 1e-5);
        assert!((xfrd.ray.d.z - d_expected.z).abs() < 1e-5);

        assert!((xfrd.ray.d.x - d_expected.x).abs() < 1e-5);
        assert!((xfrd.ray.d.y - d_expected.y).abs() < 1e-5);
        assert!((xfrd.ray.d.z - d_expected.z).abs() < 1e-5);
    }
}
