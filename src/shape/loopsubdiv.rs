use std::borrow::Borrow;
use std::rc::{Rc, Weak};
use std::hash::{Hash, Hasher};
use std::collections::HashMap;

use bbox::BBox;
use bbox::Union;
use geometry::point::Point;
use shape::mesh::Mesh;
use shape::mesh::Triangle;
use shape::shape::FromShape;
use shape::shape::IntoShape;
use shape::shape::IsShape;
use shape::shape::Shape;
use transform::transform::ApplyTransform;
use transform::transform::Transform;

fn next(i: usize) -> usize { (i + 1) % 3 }
fn prev(i: usize) -> usize { (i + 2) % 3 }

fn beta(valence: usize) -> f32 {
    if valence == 3 {
        3f32 / 16f32
    } else {
        3f32 / ((8 * valence) as f32)
    }
}

// !SPEED! This is a really poor approximation to the C++ code that was provided
// in the book. Right now we have vectors of reference counted vertices and faces.
// This means that we need to allocate a new vertex/face every time we add one to
// the array. It would be significantly faster (and increase cache coherence) if
// we had something like a RcVec that lets us create a whole reference counted
// vector and then give out Weak references to the internal elements.

struct SDVertex {
    p: Point,
    id: usize,  // We need this for ordering edges properly
    start_face: Option<Weak<SDFace>>,
    child: Option<Weak<SDVertex>>,
    regular: bool,
    boundary: bool
}

impl ::std::cmp::PartialEq for SDVertex {
    fn eq(&self, other: &SDVertex) -> bool {
        return self.id.eq(&other.id);
    }
}

impl ::std::cmp::Eq for SDVertex { }

impl Hash for SDVertex {
    fn hash<H>(&self, state: &mut H) where H : Hasher {
        self.id.hash(state);
        self.regular.hash(state);
        self.boundary.hash(state);
    }
}

impl SDVertex {
    fn new(_id: usize, _p: &Point) -> SDVertex {
        SDVertex {
            p: _p.clone(),
            id: _id,
            start_face: None,
            child: None,
            regular: false,
            boundary: false
        }
    }

    fn fold_faces<F, T>(&self, start: T, closure: F)
                        -> T where F: Fn(T, &SDFace) -> T {
        let sf = match self.start_face.clone() {
            None => return start,
            Some(_f) => _f.upgrade().unwrap()
        };

        if self.boundary {
            // Compute valence of boundary vertex
            let mut first_face : Rc<SDFace> = sf;
            let mut f : Option<Rc<SDFace>> = first_face.prev_face(self);

            while f != None {
                let face : Rc<SDFace> = f.unwrap();
                first_face = face.clone();
                f = face.prev_face(self);
            }

            f = Some(first_face);
            let mut t = start;

            while f != None {
                let face = f.unwrap();
                t = closure(t, face.as_ref());
                f = face.next_face(self);
            }

            t
        } else {
            // Compute valence of interior vertex
            let mut t = closure(start, sf.as_ref());
            let mut f = sf.next_face(self).unwrap();
            while f != sf {
                t = closure(t, f.as_ref());
                f = f.next_face(self).unwrap();
            }

            t
        }
    }

    fn valence(&self) -> usize {
        self.fold_faces(1, |nf, _| { nf + 1 })
    }

    // !SPEED! Ideally we shouldn't be allocating a vec here -- better
    // to figure out how to do something like alloca in C99...
    fn one_ring(&self) -> Vec<Point> {
        self.fold_faces(Vec::new(), |ps, nf| {
            let mut v = ps;
            if v.len() == 0 {
                v.push(nf.prev_vert(self).upgrade().unwrap().p.clone());
                v.push(nf.next_vert(self).upgrade().unwrap().p.clone());
            } else {
                v.push(nf.next_vert(self).upgrade().unwrap().p.clone());
            }
            v
        })
    }

    fn weight_boundary(&self, beta: f32) -> Point {
        let p_ring = self.one_ring();
        assert!(p_ring.len() > 0);
        (1.0 - 2.0 * beta) * &self.p + beta * &p_ring[0] + beta * &p_ring[p_ring.len() - 1]
    }

    fn weight_one_ring(&self, beta: f32) -> Point {
        let p_ring = self.one_ring();
        let mut p = (1.0 - (p_ring.len() as f32) * beta) * &self.p;
        for vi in p_ring.iter() {
            p = p + beta * vi;
        }
        p
    }
}

struct SDFace {
    v: [Weak<SDVertex>; 3],
    f: [Option<Weak<SDFace>>; 3],
    children: [Option<Weak<SDFace>>; 4],
}

impl ::std::cmp::PartialEq for SDFace {
    fn eq(&self, other: &SDFace) -> bool {
        self.v.iter().zip(other.v.iter()).fold(true, |r, (p, q)| {
            r && p.upgrade().unwrap() == q.upgrade().unwrap()
        })
    }
}

impl SDFace {
    fn new(v1: Weak<SDVertex>, v2: Weak<SDVertex>, v3: Weak<SDVertex>) -> SDFace {
        SDFace {
            v: [v1.clone(), v2.clone(), v3.clone()],
            f: [None, None, None],
            children: [None, None, None, None]
        }
    }

    fn vnum(&self, v: &SDVertex) -> usize {
        for i in 0..3 {
            if (*self.v[i].upgrade().unwrap()).borrow() == v {
                return i;
            }
        }

        panic!("Basic logic error in SDFace::vnum()");
    }

    fn next_face(&self, v: &SDVertex) -> Option<Rc<SDFace>> {
        self.f[self.vnum(v)].clone().map(|fr| fr.upgrade().unwrap())
    }

    fn prev_face(&self, v: &SDVertex) -> Option<Rc<SDFace>> {
        self.f[prev(self.vnum(v))].clone().map(|fr| fr.upgrade().unwrap())
    }

    fn next_vert(&self, v: &SDVertex) -> Weak<SDVertex> {
        self.v[next(self.vnum(v))].clone()
    }

    fn prev_vert(&self, v: &SDVertex) -> Weak<SDVertex> {
        self.v[prev(self.vnum(v))].clone()
    }
}

struct SDEdge {
    v: [Weak<SDVertex>; 2],
    f: [Option<Weak<SDFace>>; 2],
    f0_edge_num: usize
}

impl ::std::cmp::PartialEq for SDEdge {
    fn eq(&self, other: &SDEdge) -> bool {
        self.v[0].upgrade() == other.v[0].upgrade() &&
        self.v[1].upgrade() == other.v[1].upgrade()
    }
}

impl ::std::cmp::Eq for SDEdge { }

impl Hash for SDEdge {
    fn hash<H>(&self, state: &mut H) where H : Hasher {
        self.v[0].upgrade().unwrap().hash(state);
        self.v[1].upgrade().unwrap().hash(state);
    }
}

impl SDEdge {
    fn new(v1: Weak<SDVertex>, v2: Weak<SDVertex>) -> SDEdge {
        let (min_v, max_v) =
            if v1.upgrade().unwrap().id < v2.upgrade().unwrap().id {
                (v1.clone(), v2.clone())
            } else {
                (v2.clone(), v1.clone())
            };

        SDEdge {
            v: [min_v, max_v],
            f: [None, None],
            f0_edge_num: 4
        }
    }
}

pub struct LoopSubdiv {
    shape: Shape,
    n_levels: usize,
    vertices: Vec<Rc<SDVertex>>,
    faces: Vec<Rc<SDFace>>,
    max_vert_id: usize
}

impl LoopSubdiv {
    pub fn new(o2w: Transform, w2o: Transform, ro: bool,
               vertex_indices: &[usize], points: &[Point], nl: usize)
               -> LoopSubdiv {
        // Allocate vertices
        let mut vert_id = 0;
        let mut verts = {
            let mut vs = Vec::new();

            for p in points {
                vs.push(Rc::new(SDVertex::new(vert_id, p)));
                vert_id += 1;
            }

            vs
        };

        // Allocate faces
        debug_assert_eq!((vertex_indices.len() % 3), 0);
        let num_faces = vertex_indices.len() / 3;
        let mut faces: Vec<Rc<SDFace>> = {
            let mut vert_idxs = vertex_indices.iter();

            (0..num_faces).map(|_| {
                let v0 = *vert_idxs.next().unwrap();
                let v1 = *vert_idxs.next().unwrap();
                let v2 = *vert_idxs.next().unwrap();
                let f = Rc::new(SDFace::new(
                    Rc::downgrade(&verts[v0]),
                    Rc::downgrade(&verts[v1]),
                    Rc::downgrade(&verts[v2])));

                Rc::get_mut(&mut verts[v0]).unwrap().start_face = Some(Rc::downgrade(&f));
                Rc::get_mut(&mut verts[v1]).unwrap().start_face = Some(Rc::downgrade(&f));
                Rc::get_mut(&mut verts[v2]).unwrap().start_face = Some(Rc::downgrade(&f));

                f
            }).collect()
        };

        // Set neighbor pointers in faces
        let mut edges: HashMap<(usize, usize), SDEdge> = HashMap::new();
        for f in faces.iter_mut() {
            for edge_num in 0..3 {
                let v0 = edge_num;
                let v1 = next(edge_num);

                // Update neighbor pointer for edge_num
                let key = {
                    let id1 = f.v[v0].clone().upgrade().unwrap().id;
                    let id2 = f.v[v1].clone().upgrade().unwrap().id;
                    if id1 < id2 { (id1, id2) } else { (id2, id1) }
                };

                if edges.contains_key(&key) {
                    {
                        let e = edges.get_mut(&key).unwrap();
                        assert!(e.f0_edge_num < 4);

                        // Handle previously seen edge
                        Rc::get_mut(&mut e.f[0].as_mut().unwrap().upgrade().unwrap())
                            .unwrap().f[e.f0_edge_num] = Some(Rc::downgrade(f));
                        Rc::get_mut(f).as_mut().unwrap().f[edge_num] = e.f[0].clone();
                    }
                    edges.remove(&key);
                } else {
                    // Handle new edge
                    let mut e = SDEdge::new(f.v[v0].clone(), f.v[v1].clone());
                    e.f[0] = Some(Rc::downgrade(f));
                    e.f0_edge_num = edge_num;
                    edges.insert(key, e);
                }
            }
        }

        // Finish vertex initialization
        for v in verts.iter_mut() {
            let boundary = {
                let sf = {
                    match v.start_face.clone() {
                        None => continue,
                        Some(_f) => _f.upgrade().unwrap()
                    }
                };

                let mut f = sf.clone();
                let mut is_boundary = false;
                loop {
                    f = match f.next_face((*v).borrow()) {
                        None => {
                            is_boundary = true;
                            break;
                        },

                        Some(_f) => _f
                    };

                    if f == sf { break };
                }

                is_boundary
            };

            Rc::get_mut(v).unwrap().boundary = boundary;
            Rc::get_mut(v).unwrap().regular =
                (!v.boundary && v.valence() == 6) || (v.boundary && v.valence() == 4);
        }

        LoopSubdiv {
            shape: Shape::new(o2w, w2o, ro),
            n_levels: nl,
            vertices: verts,
            faces: faces,
            max_vert_id: vert_id
        }
    }
}

impl IntoShape for LoopSubdiv { }
impl FromShape<LoopSubdiv> for Mesh { }
impl FromShape<LoopSubdiv> for LoopSubdiv { }

impl<'a> IsShape<'a, Mesh> for LoopSubdiv {
    fn get_shape(&'a self) -> &'a Shape { &self.shape }
    fn object_bound(&self) -> BBox {
        self.vertices.iter().fold(BBox::new(), |b, v| b.unioned_with_ref(&v.p))
    }

    fn world_bound(&self) -> BBox {
        let o2w = &self.get_shape().object2world;
        self.vertices.iter().fold(BBox::new(), |b, v| b.unioned_with(o2w.t(&v.p)))
    }

    // Cannot intersect meshes directly.
    fn can_intersect(&self) -> bool { false }

    fn refine(&'a mut self) -> Vec<Mesh> {
        let mut vtx_id = self.max_vert_id;
        for i in 0..self.n_levels {
            // Update f and v for next level of subdivision
            // let new_faces = Vec::new();
            let mut new_vertices = Vec::new();

            // Allocate next level of children in mesh tree
            for vtx in self.vertices.iter_mut() {
                // Determine new vertex position
                let p = if vtx.boundary {
                    // Apply boundary rule for even vertex
                    vtx.weight_boundary(1f32 / 8f32)
                } else {
                    // Apply one-ring rule for even vertex
                    if vtx.regular {
                        vtx.weight_one_ring(1f32 / 16f32)
                    } else {
                        vtx.weight_one_ring(beta(vtx.valence()))
                    }
                };

                let mut new_vtx = Rc::new(SDVertex::new(vtx_id, &p));
                Rc::get_mut(&mut new_vtx).unwrap().boundary = vtx.boundary;
                Rc::get_mut(&mut new_vtx).unwrap().regular = vtx.regular;

                Rc::get_mut(vtx).unwrap().child = Some(Rc::downgrade(&new_vtx));
                new_vertices.push(new_vtx);
            }

            // Update vertex positions and create new edge vertices
            // Update new mesh topology
            // Prepare for next level of subdivision
        }

        // Push vertices to limit surface
        // Compute vertex tangents on limit surface
        // Create TriangleMesh from subdivision mesh

        self.max_vert_id = vtx_id;
        vec![]
    }
}

////////////////////////////////////////////////////////////////////////////////
//
// !FIXME! We have to wait for https://github.com/rust-lang/rust/issues/30658
// to be fixed before we can implement these fixes...

#[cfg(test)]
mod tests {
    use super::*;

    use geometry::point::Point;
    use shape::shape::IsShape;
    use transform::transform::Transform;

    // Tetrahedron
    static TET_PTS : [Point; 4] =
        [Point { x: 0.0, y: 0.0, z: 0.0 },
         Point { x: 1.0, y: 0.0, z: 0.0 },
         Point { x: 0.0, y: 1.0, z: 0.0 },
         Point { x: 0.0, y: 0.0, z: 1.0 }];
    static TET_TRIS : [usize; 12] =
        [ 0, 3, 2, 0, 1, 2, 0, 3, 1, 1, 2, 3 ];

    #[ignore]
    #[test]
    fn it_can_be_created() {
        let subdiv = LoopSubdiv::new(Transform::new(), Transform::new(), false,
                                     &TET_TRIS, &TET_PTS, 1);
        assert_eq!(subdiv.n_levels, 1);
        assert_eq!(subdiv.vertices.len(), 4);
        assert_eq!(subdiv.faces.len(), 4);
    }

    #[ignore]
    #[test]
    fn it_has_object_space_bounds() {
        unimplemented!();
    }

    #[ignore]
    #[test]
    fn it_has_world_space_bounds() {
        unimplemented!();
    }

    #[ignore]
    #[test]
    fn it_can_be_refined() {
        unimplemented!();
    }

    #[ignore]
    #[test]
    fn it_cant_be_intersected() {
        assert!(!LoopSubdiv::new(Transform::new(), Transform::new(), false,
                                 &TET_TRIS, &TET_PTS, 1).can_intersect());
    }
}
