extern crate scoped_threadpool;
extern crate num_cpus;

use camera;
use integrator;
use integrator::Integrator;
use ray;
use rng::RNG;
use rng::PseudoRNG;
use renderer;
use renderer::Renderer;
use sampler;
use scene;
use scoped_threadpool::Pool;
use spectrum::Spectrum;
use intersection;

use std::ops::BitAnd;
use std::sync::{Mutex, MutexGuard, Arc};

pub struct SamplerRenderer {
    sampler: sampler::Sampler,
    camera: camera::Camera,
    surface_integrator: integrator::SurfaceIntegrator,
    volume_integrator: integrator::VolumeIntegrator,
    // SamplerRenderer Private Data
}

impl SamplerRenderer {
    pub fn new(sampler : sampler::Sampler, cam : camera::Camera,
               surf: integrator::SurfaceIntegrator,
               vol: integrator::VolumeIntegrator) -> SamplerRenderer {
        SamplerRenderer {
            sampler: sampler,
            camera: cam,
            surface_integrator: surf,
            volume_integrator: vol
        }
    }

    pub fn new_empty() -> SamplerRenderer {
        SamplerRenderer {
            sampler: sampler::Sampler,
            camera: camera::Camera::new(512, 512),
            surface_integrator: integrator::SurfaceIntegrator,
            volume_integrator: integrator::VolumeIntegrator,
        }
    }
}

struct SamplerRendererTaskData<'a> {
    scene: &'a scene::Scene,
    renderer: &'a mut SamplerRenderer,
    sample: &'a mut sampler::Sample
}

impl<'a> SamplerRendererTaskData<'a> {
    fn new(scene: &'a scene::Scene,
           renderer: &'a mut SamplerRenderer,
           sample: &'a mut sampler::Sample) ->
        SamplerRendererTaskData<'a> {
            SamplerRendererTaskData {
                scene: scene,
                renderer: renderer,
                sample: sample
            }
        }
}

fn run_task<'a, 'b>(task_data : &'b Arc<Mutex<&'a mut SamplerRendererTaskData<'a>>>,
            task_idx: i32, num_tasks: i32) {
    // Get sub-sampler for SamplerRendererTask
    let mut sampler = {
        let mut data : MutexGuard<'b, &'a mut SamplerRendererTaskData<'a>> =
            task_data.lock().unwrap();
        if let Some(s) = data.renderer.sampler.get_sub_sampler(task_idx, num_tasks)
        { s } else { return }
    };
    
    let scene = {
        let mut data : MutexGuard<'b, &'a mut SamplerRendererTaskData<'a>> =
            task_data.lock().unwrap();
        data.scene
    };

    // Declare local variables used for rendering loop
    let mut rng = PseudoRNG::new(task_idx);
    
    // Allocate space for samples and intersections
    let max_samples = sampler.maximum_sample_count() as usize;
    let mut samples : Vec<sampler::Sample> = {
        let mut data : MutexGuard<'b, &'a mut SamplerRendererTaskData<'a>> =
            task_data.lock().unwrap();
        (0..max_samples).map(|_| data.sample.clone()).collect()
    };
    let mut rays : Vec<ray::RayDifferential> = Vec::with_capacity(max_samples);
    let mut l_s : Vec<Spectrum> = Vec::with_capacity(max_samples);
    let mut t_s : Vec<Spectrum> = Vec::with_capacity(max_samples);
    let mut isects : Vec<intersection::Intersection> = Vec::with_capacity(max_samples);

    // Get samples from Sampler and update image
    loop {
        sampler.get_more_samples(&mut samples, &mut rng);
        let sample_count = samples.len();
        if (sample_count == 0) { break; }

        // Generate camera rays and compute radiance along rays
        let _ : Vec<_> = (0..sample_count).map(|i| {
            // Find camera ray for sample[i]
            let (ray_weight, mut ray) = {
                let mut data : MutexGuard<'b, &'a mut SamplerRendererTaskData<'a>> =
                    task_data.lock().unwrap();
                data.renderer.camera.generate_ray_differential(&(samples[i]))
            };

            ray.scale_differentials(1.0f32 / sampler.samples_per_pixel().sqrt());

            // Evaluate radiance along camera ray
            if (ray_weight > 0f32) {
                let mut ts: Option<Spectrum> = None;
                let mut isect: Option<intersection::Intersection> = None;

                // !FIXME! I think this synchronization is a bit too coarse grained
                let ls = {
                    let mut data : MutexGuard<'b, &'a mut SamplerRendererTaskData<'a>> =
                        task_data.lock().unwrap();
                    ray_weight * data.renderer.li(scene, &ray, &(samples[i]), &mut rng, &mut isect, &mut ts)
                };

                if (!ls.is_valid()) { panic!("Invalid radiance value!"); }
                l_s.push(ls);

                if let Some(ts_val) = ts {
                    t_s.push(ts_val);
                } else {
                    t_s.push(Spectrum::from_value(0f32));
                }
                
                if let Some(isect_val) = isect {
                    isects.push(isect_val);
                } else {
                    // Empty intersection
                    isects.push(intersection::Intersection);
                }
            } else {
                l_s.push(Spectrum::from_value(0f32));
                t_s.push(Spectrum::from_value(0f32));
                // Empty intersection
                isects.push(intersection::Intersection);
            }
        }).collect();

        // Report sample results to Sampler, add contributions to image
    }
    // Clean up after SamplerRendererTask is done with its image region

    // !DEBUG!
    let sample = {
        let mut data : MutexGuard<'b, &'a mut SamplerRendererTaskData<'a>> = task_data.lock().unwrap();
        data.sample.idx
    };
    println!("Got sample {} fo task {} of {}", sample, task_idx, num_tasks);
}

impl Renderer for SamplerRenderer {
    fn render(&mut self, scene : &scene::Scene) {
        // Allow integrators to do preprocessing for the scene
        self.surface_integrator.preprocess(scene, &(self.camera));
        self.volume_integrator.preprocess(scene, &(self.camera));

        // Allocate and initialize sample
        let mut sample = sampler::Sample::new(&(self.sampler), &(self.surface_integrator),
                                              &(self.volume_integrator), &scene, 1);

        // Create and launch SampleRendererTasks for rendering image
        {
            let num_cpus = num_cpus::get() as i32;
            let num_pixels = self.camera.film().num_pixels();

            let num_tasks = (|x : i32| {
                31 - (x.leading_zeros() as i32) + (if 0 == x.bitand(x - 1) { 0 } else { 1 })
            }) (::std::cmp::max(32 * num_cpus, num_pixels / (16 * 16)));

            let mut task_data = SamplerRendererTaskData::new(scene, self, &mut sample);
            let task_data_async = Arc::new(Mutex::new(&mut task_data));

            println!("Running {} tasks on pool with {} cpus", num_tasks, num_cpus);
            Pool::new(num_cpus as u32).scoped(|scope| {
                let _ : Vec<_> = (0..num_tasks).map(|i| {
                    let data = task_data_async.clone();
                    unsafe { scope.execute(move || run_task(&data, i, num_tasks)); }
                }).collect();
            });
        }

        // Clean up after rendering and store final image    
    }

    fn li<T:RNG>(
        &self, scene: &scene::Scene, ray: &ray::RayDifferential,
        sample: &sampler::Sample, rng: &mut T,
        isect: &mut Option<intersection::Intersection>,
        spect: &mut Option<Spectrum>) -> Spectrum {
        Spectrum
    }

    fn transmittance<T:RNG>(
        &self, scene: &scene::Scene, ray: &ray::RayDifferential,
        sample: &sampler::Sample, rng: &mut T) -> Spectrum {
        Spectrum
    }

    // Rnderer Interface
}
