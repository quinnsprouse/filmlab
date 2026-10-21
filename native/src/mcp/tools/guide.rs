use gpui_kit::AsyncApp;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Answer, Output, args, edit};
use crate::ui::driver::Driver;

const WORKFLOW: &str = "\
Filmlab: LightCraft's raw developer with FilmVision film (negative and print LUTs, halation, grain, blur, dust), live in \
a window the user may be watching. Follow the user's intent.
Start: status; photos {action:\"folder\",args:{path}}; photos {action:\"list\"}. Address photos by id; photos open \
only to show the user one.
Look first: look preview; look analyze for numbers or scopes; region [x0,y0,x1,y1] for an area at full detail (grain, \
sharpness, halos, small changes).
edit: values (ids: guide controls) and film in one step. It answers with the effect and only the numbers that moved, at \
512 px; watch adds current readings. After context loss, read status state and look analyze again.
Tone curve: edit curve, levels 0-255 (guide curves).
fit: one control until a measure reaches to, or another photo's (like); max_delta bounds it. look trials: alternatives \
side by side, nothing saved; commit the chosen one with edit if_state.
edit paste: a look onto other photos without a profile, keeping each one's exposure and white balance; match only for \
the same scene and light. Profiles are the user's: save or delete only when asked.
Numbers diagnose; the scene and intent decide. End with look compare from \"before\" and full-detail checks of skin, \
skies and highlights. A conflict means read again. Give each change a new request_id; if its answer is lost or times \
out, send it again with the same id and retry: true, which returns its answer or says the outcome is unknown (then \
read status state) and never runs it twice.";

const FILM: &str = "\
FilmVision film: edit film {negative, print} (ids below; negative null turns the film off). Default pairing: Vision3 250D \
with Print SA1 Kodak 2393. gauge 16/35/65/120 sets FilmVision's grain, halation radius and blur for that format (65 fine, \
16 heavy), a jump rather than a slide. More fields: guide schema film. Expose and balance first, then judge the film; \
light contrast/whites/blacks act as FilmVision's contrast and scan levels.";

const CURVES: &str = "\
Tone curve, levels 0-255, after the film print: regions (curve.highlights, lights, darks, shadows, splits; values), \
then rgb, then red, green, blue.
edit curve, one step with values and film: {rgb: [[in,out],...]} replaces points; {rgb: {\"64\": \"+8\", \"255\": 240}} \
moves the point within 3 levels or adds one on the line; {red: null} linear; {preset: \"Medium Contrast\"} (Linear, \
Strong Contrast, or a curve the user saved). Keep a curve only when the user asks: profiles save {name, groups: \
[\"toneCurve\"]}; it then shows in the curve menu and as state's preset.
Recipes: lift shadows rgb {\"46\":69,\"140\":145}; fade blacks {\"0\":26,\"77\":82}; Medium Contrast; soften \
highlights {\"153\":153,\"255\":230}; split red {\"158\":168}, blue [[0,18],[128,125],[255,237]].
Answers add curve: tones in: was→now, black, white, slope at the photo's p10/p50/p90 (over 1: more contrast), neutral \
(a grey's a* b*), notes (banding, flattened, reversal, crossover, matte, highlight edge) with pixel shares; the scene \
decides.
look analyze scopes [\"curve\"]: points, map, hist in/out; region (a face, a sky): its in/out p5 p50 p95, where a point \
goes. image \"curve\" draws it, from a ghost. trials and compare take curve; fit control \"curve.rgb@128\".";

const MASKS: &str = "\
Local adjustments are masks, through edit action command: mask.add {kind: linear|radial|luminanceRange|colorRange|subject|\
sky|background} returns activeMask (an id); mask.update {id, component, shape}; mask.adjust {id, values: {exposure: 0.5}}; \
mask.refine, mask.invert, mask.delete. guide schema command.mask lists them with params. A new mask changes nothing until \
it's adjusted; check the area with look preview region.";

const CROP: &str = "\
Crop through edit action command: crop.aspect {aspect: original|free|1x1|4x5|2x3|16x9|[w,h]}, crop.straighten {angle}, \
crop.autoStraighten, crop.rotateAspect, crop.reset; guide schema command.crop lists the rest. Previews show the cropped \
frame.";

const METRICS: &str = "\
Every edit's numbers (glance): p5 p50 p95 p99 code luma 0-1; white% black% clipped (any channel >= 254, all <= 1); \
cast_a cast_b, the mean a* b* of near-neutral mid tones (absent under 2% of the frame); chroma90, Lab chroma p90. An \
edit's delta shows a number when it moves at least: luma .001, clip .01%, cast .1, chroma .1. Smaller changes can still \
show at full size.
fit and watch also take p1 p25 p75 mean chroma50, zone colour sh/mid/hi.a and .b, and texture.
Scopes (look analyze; only those asked for, with the sample size): glance; zones: shadows/mid tones/highlights by luma (under .3, .3-.7, over .7), each share %, mean luma \
y, mean a* b* and chroma p90 c90. hues: per hue sector with half a percent of the frame or more, share, mean hue h and \
chroma c50/c90. skin: against the vectorscope skin line (Cb/Cr, ~123°): with a region on a face it reads every mid tone \
there, without one only skin-coloured candidates (walls and wood too); dh is the mean angle off the line (negative \
toward magenta, positive toward yellow-green), spread how far most stray, c50/c90 saturation. clip: R G B pixel counts \
at the top and bottom. tail: luma p99, p99.9, max and pixels over .9/.95/.98. wave: 8 vertical strips' luma p1 p50 \
p99, left to right. texture: edge and grain energy, for comparing a region before and after. curve: the tone curve's \
points, map, hist in/out, slope, neutral and notes (guide curves). All diagnose; none is a target on its own.";

/// Each rare operation's arguments, for `guide schema`.
const SCHEMAS: &[(&str, &str)] = &[
    ("photos.folder", "{path (~ ok), wait?: bool, default true}"),
    ("photos.list", "{filter?: all|picks|rejected|edited|unedited, offset?, limit? (default 50), detail?: bool adds camera, exposure, size}"),
    ("photos.flags", "{ids: [id], flag?: pick|reject|none, rating?: 0-5, label?: string (\"\" clears)}"),
    ("photos.open", "{id}: shows the photo in Develop for the user and waits for it to render"),
    ("photos.export", "{ids?: [id] (default the open photo), format?: jpeg|png|tiff, long_edge?, quality?: 1-100 (90), folder? (~ ok, made if missing; default <photo folder>/Exports), name? (one photo)}"),
    ("view", "{mode?: library|develop, tool?: edit|crop|heal|masks, zoom?: \"fit\"|\"100\"|\"2\", at?: [x, y] 0-1, compare?: before|after, select?: [id], rail?: bool}"),
    ("profiles", "list {}; apply {photo?, id or name, amount?: 0-200 (100)}; save {photo?, name, groups?: [group]} and delete {id or name} only when the user asks"),
    ("edit.actions", "command {name, params} (guide schema command.<prefix>); undo, redo; go_to {step}; snapshot.save|restore|delete {name}"),
    ("edit.paste", "paste: {from? (default the open photo), groups?: [look | all | film, profile, treatment, whiteBalance, light, toneCurve, color, colorMixer, colorGrading, effects, vignette, grain, detail, optics, geometry, crop, masks, spots, redEye, lensBlur, calibration], amount?: 1-200 (film's stocks switch at 50), match?: [exposure, wb]}; photo: the targets"),
    ("film", "{negative?, print?, utility? (ids from guide film; null turns it off), gauge?: 16|35|65|120, halation?: {on, amount 0-2, hue 0-1, sat 0-1, radius 1-10, highlights_only}, grain?: {amount 0-1, size 0-4, softness 0-1, defocus 0.5-1}, blur?: 0-0.2, dust?: {on, size 1.2-4, softness 0-0.5}}"),
    ("look", "args: {quality?: 30-95 (JPEG), values?: {control: value} and curve? (compare: tried on top of to, saving nothing), detail?: bool (compare: every statistic)}; edge: image long edge (preview 768, sheets fit the count, compare 512); analyze image \"curve\" with scopes [\"curve\"] draws the curve, from a ghost"),
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Guide {
    topic: String,
    filter: Option<String>,
}

fn text(t: &str) -> Answer {
    Ok(Output { text: t.to_string(), images: Vec::new(), error: false })
}

pub fn guide(driver: &Driver, cx: &mut AsyncApp, value: Value) -> Answer {
    let p: Guide = args(value)?;
    let filter = p.filter.as_deref().map(str::trim).filter(|f| !f.is_empty());
    if filter.is_some() && ["workflow", "curves", "masks", "crop", "metrics"].contains(&p.topic.as_str()) {
        return Err(format!("{} is one text; filter narrows controls, film and schema", p.topic));
    }
    match p.topic.as_str() {
        "workflow" => text(WORKFLOW),
        "curves" => text(CURVES),
        "masks" => text(MASKS),
        "crop" => text(CROP),
        "metrics" => text(METRICS),
        "controls" => edit::catalog(driver, cx, json!({"kind": "controls", "filter": filter})),
        "film" => {
            let stocks = edit::catalog(driver, cx, json!({"kind": "stocks", "filter": filter}))?;
            text(&format!("{FILM}\n{}", stocks.text))
        }
        "schema" => {
            let wanted = filter.ok_or_else(|| format!("give filter: an operation ({}) or command.<prefix>", SCHEMAS.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")))?;
            if let Some(prefix) = wanted.strip_prefix("command") {
                return edit::catalog(driver, cx, json!({"kind": "commands", "filter": prefix.trim_start_matches('.')}));
            }
            let found: Vec<String> = SCHEMAS.iter().filter(|(n, _)| n.starts_with(wanted) || wanted.starts_with(n)).map(|(n, s)| format!("{n}: {s}")).collect();
            if found.is_empty() {
                return Err(format!("no schema for {wanted}; operations: {}, command.<prefix>", SCHEMAS.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")));
            }
            text(&found.join("\n"))
        }
        other => Err(format!("no guide on {other}; topics: workflow, film, controls, curves, masks, crop, metrics, schema")),
    }
}
