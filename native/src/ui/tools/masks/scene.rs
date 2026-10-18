use super::geometry::{pin_point, radial_edge_handles, radial_rotate_handle, radial_ring};
use super::model::{Mask, Pt, Radial, Shape};
use super::view::ViewSpace;

const RING_STEPS: usize = 64;

#[derive(Clone)]
pub struct Grip {
    pub mask: u32,
    pub component: usize,
    pub handle: usize,
    pub at: Pt,
    pub shape: Shape,
}

#[derive(Clone)]
pub struct RadialDraw {
    pub ring: Vec<Pt>,
    pub feather_ring: Vec<Pt>,
    pub handles: Vec<Pt>,
    pub rotate: Pt,
}

#[derive(Clone)]
pub struct LinearDraw {
    pub start: Pt,
    pub end: Pt,
    pub middle: Pt,
}

#[derive(Clone)]
pub struct Pin {
    pub at: Pt,
    pub selected: bool,
}

#[derive(Clone, Default)]
pub struct Scene {
    pub radials: Vec<RadialDraw>,
    pub linears: Vec<LinearDraw>,
    pub pins: Vec<Pin>,
    pub grips: Vec<Grip>,
}

fn grip(mask: u32, component: usize, handle: usize, at: Pt, shape: &Shape) -> Grip {
    Grip { mask, component, handle, at, shape: shape.clone() }
}

fn radial_draw(space: &ViewSpace, shape: &Shape, r: &Radial, mask: u32, component: usize, scene: &mut Scene) {
    let frame = (space.width, space.height);
    let ring = radial_ring(r, frame, 1.0, RING_STEPS);
    let feather_ratio = (1.0 - r.feather / 100.0).max(0.05);
    let feather = radial_ring(r, frame, feather_ratio, RING_STEPS);
    let edges = radial_edge_handles(r, frame);
    let mut all = ring;
    all.extend(feather);
    all.extend(edges);
    all.push(radial_rotate_handle(r, frame));
    let view = space.to_view(&all);
    if view.len() != all.len() {
        return;
    }
    let handles = view[RING_STEPS * 2..RING_STEPS * 2 + 4].to_vec();
    let rotate = view[view.len() - 1];
    for (position, at) in handles.iter().chain(std::iter::once(&rotate)).enumerate() {
        scene.grips.push(grip(mask, component, position + 1, *at, shape));
    }
    scene.radials.push(RadialDraw { ring: view[..RING_STEPS].to_vec(), feather_ring: view[RING_STEPS..RING_STEPS * 2].to_vec(), handles, rotate });
}

fn linear_draw(space: &ViewSpace, shape: &Shape, mask: u32, component: usize, scene: &mut Scene) {
    let Shape::Linear { start, end } = shape else { return };
    let middle = pin_point(shape).unwrap_or(*start);
    let view = space.to_view(&[*start, *end, middle]);
    let [s, e, m] = view[..] else { return };
    scene.grips.push(grip(mask, component, 1, s, shape));
    scene.grips.push(grip(mask, component, 2, e, shape));
    scene.linears.push(LinearDraw { start: s, end: e, middle: m });
}

pub fn build_scene(masks: &[Mask], active: Option<u32>, space: &ViewSpace) -> Scene {
    let mut scene = Scene::default();
    let mut pinned: Vec<(u32, usize, &Shape, Pt)> = Vec::new();
    for mask in masks {
        for (component, c) in mask.components.iter().enumerate() {
            let selected = Some(mask.id) == active;
            match &c.shape {
                Shape::Radial(r) if selected => radial_draw(space, &c.shape, r, mask.id, component, &mut scene),
                Shape::Linear { .. } if selected => linear_draw(space, &c.shape, mask.id, component, &mut scene),
                _ => {}
            }
            if let Some(norm) = pin_point(&c.shape) {
                pinned.push((mask.id, component, &c.shape, norm));
            }
        }
    }
    let norms: Vec<Pt> = pinned.iter().map(|p| p.3).collect();
    let views = if norms.is_empty() { Vec::new() } else { space.to_view(&norms) };
    for ((mask, component, shape, _), at) in pinned.iter().zip(views) {
        scene.pins.push(Pin { at, selected: Some(*mask) == active });
        scene.grips.push(grip(*mask, *component, 0, at, shape));
    }
    scene
}
