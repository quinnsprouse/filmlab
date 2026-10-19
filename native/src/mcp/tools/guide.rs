use serde_json::Value;

use super::{Answer, Output};

const WORKFLOW: &str = "\
Filmlab = LightCraft's raw developer + FilmVision Pro's film emulation (negative LUT -> print LUT, with FilmVision's texture: \
halation, grain, film blur, dust). The window shows every change live; the user may be watching.
Loop: status -> open_folder(path) -> list_photos -> open_photo(id) -> preview (look) + analyze (measure) -> change -> preview again.
Changes: set_controls for sliders (ids from catalog controls; relative strings like \"+0.3\" work), set_film for the film look, \
command for crop / masks / healing / white balance and anything else LightCraft can do (catalog commands). Each change is one \
undo step (history) and is saved to the photo's XMP sidecar automatically.
Look closely: preview region=[x0,y0,x1,y1] (0-1 of the cropped frame) renders that area at full detail; analyze gives numbers \
for the same region. view drives the window (zoom, tool, mode) when you want the user to see something.
Other photos: get_edit, set_controls, set_film, command, history, snapshots, profiles, preview and analyze take photo=<id> to \
work on that photo without opening it (its own history; saved like any edit; status lists them under sessions). Without photo \
they act on the open photo. Calls on different photos run at the same time; calls on one photo run in order.
Keep states: snapshots save/restore named states of one photo; profiles save a look to apply to other photos (amount 0-200%).
Finish with export when asked. Prefer small steps and check each with preview or analyze.";

const FILM: &str = "\
The film look is FilmVision: set_film negative + print (stock ids from catalog stocks; negative=null removes the film). \
Default pairing: negative \"Negative/Vision3 250D\" with print \"Print/I- Standard/Print SA1 Kodak 2393\". \
gauge 16/35/65/120 applies FilmVision's own grain, halation radius and film blur for that format (65 = fine, 16 = heavy). \
halation: amount 0-2 (FilmVision stock 0.5), hue 0-1 (0.45 warm red-orange), sat 0-1, radius, highlights_only. \
grain: amount, size, softness, defocus. dust: on, size, softness. blur: film softness. \
The film stage works best on a well-exposed, neutral scene: set exposure and white balance first, then judge the film. \
Light contrast/whites/blacks act before and after the film as FilmVision's contrast and scan levels.";

const CONTROLS: &str = "\
catalog controls lists every slider: id, range, default, step, by section (light, color, wb, effects, detail, optics, ...). \
get_edit shows the values that differ from default. set_controls takes many ids at once as one step. \
Exposure is in stops; most others are -100..100. White balance: wb.temp / wb.tint (or command develop.wb for as-shot/auto).";

const MASKS: &str = "\
Local adjustments are masks: command mask.add {kind: linear|radial|luminanceRange|colorRange|subject|sky|background} returns \
activeMask (an id). Shape it with mask.update {id, component, shape}, adjust it with mask.adjust {id, values: {exposure: 0.5, \
...}}, refine edges with mask.refine, invert with mask.invert, delete with mask.delete. catalog commands filter=mask lists all \
of them with params. Check the result with preview (a region around the masked area).";

const CROP: &str = "\
Crop and straighten with commands: crop.aspect {aspect: \"original\"|\"free\"|\"1x1\"|\"4x5\"|\"2x3\"|\"16x9\"|[w,h]}, \
crop.straighten {angle} or develop.set {values:{\"crop.angle\": deg}}, crop.autoStraighten, crop.rotateAspect, crop.reset. \
catalog commands filter=crop lists the rest. preview always shows the cropped frame; view tool=crop shows the crop UI.";

const QA: &str = "\
Targets that read as well-made film photos (display values 0-1 from analyze): whites p99.5 ~0.87-0.92 (don't leave them \
dull at 0.7); shadows p5 >= ~0.07 unless the scene is meant to be dark; clip% white < ~0.5 unless there are speculars; \
neutral_cast a/b near 0, slightly warm (b +1..+4) for daylight; skin natural and warm, never magenta. Warm, organic film \
tones, not oversaturated (chroma p90 roughly 20-45). Keep moody scenes moody. Level horizons (crop.autoStraighten). \
Always compare against before=true and inspect faces/skies at full detail with a region preview.";

pub fn guide(value: Value) -> Answer {
    let topic = value["topic"].as_str().unwrap_or("workflow");
    let text = match topic {
        "workflow" => WORKFLOW,
        "film" => FILM,
        "controls" => CONTROLS,
        "masks" => MASKS,
        "crop" => CROP,
        "qa" => QA,
        other => return Err(format!("no guide on {other}; topics: workflow, film, controls, masks, crop, qa")),
    };
    Ok(Output { text: text.to_string(), images: Vec::new(), error: false })
}
