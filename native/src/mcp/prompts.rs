use rmcp::model::{GetPromptResult, Prompt, PromptArgument, PromptMessage, Role};
use serde_json::{Map, Value};

struct Template {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    arguments: &'static [(&'static str, &'static str, bool)],
    text: fn(&Map<String, Value>) -> String,
}

fn arg<'a>(args: &'a Map<String, Value>, name: &str) -> Option<&'a str> {
    args.get(name).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

fn grade(args: &Map<String, Value>) -> String {
    let photo = arg(args, "photo").map_or("the photo open in Filmlab (status shows it)".to_string(), |p| format!("photo {p}"));
    let look = arg(args, "look").map_or(String::new(), |l| format!(" The look to aim for: {l}."));
    format!(
        "Grade {photo} in Filmlab.{look} First read guide workflow and guide qa. Then loop: preview and analyze, change one \
         thing (exposure and white balance first, then the film look with set_film, then local fixes with masks), check \
         again. Save a snapshot whenever a state is worth keeping. Inspect faces, skies and highlights at full detail with a \
         region preview. Finish with a before/after comparison (preview before=true) and a short note of what you changed and \
         why."
    )
}

fn cull(args: &Map<String, Value>) -> String {
    let folder = arg(args, "folder").map_or("the folder open in Filmlab".to_string(), |f| format!("the folder {f} (open_folder)"));
    let keep = arg(args, "keep").map_or(String::new(), |k| format!(" Keep about {k}."));
    format!(
        "Cull {folder}.{keep} Look at each photo with preview (small, around 512 px) and analyze; flag the best as pick and the \
         failures (missed focus, blur, closed eyes, duplicates) as reject with set_flags, and rate keepers 1-5. Compare near \
         duplicates side by side before choosing. End with list_photos filter=picks and one line per pick saying why."
    )
}

fn match_look(args: &Map<String, Value>) -> String {
    let source = arg(args, "source").unwrap_or("the photo open in Filmlab");
    let targets = arg(args, "targets").map_or("the other photos in the folder".to_string(), |t| format!("photos {t}"));
    format!(
        "Carry the look of {source} to {targets}. Save its look as a profile (profiles action=save, default look groups), \
         apply it to each target (open_photo, profiles action=apply), then fix each one's own exposure and white balance so \
         they sit together, checking every photo with preview and analyze against the source. Report what differed."
    )
}

const TEMPLATES: &[Template] = &[
    Template {
        name: "grade",
        title: "Grade a photo",
        description: "Edit one photo to a finished film look in a preview/measure loop.",
        arguments: &[("photo", "photo id; default the open photo", false), ("look", "the look to aim for", false)],
        text: grade,
    },
    Template {
        name: "cull",
        title: "Cull a folder",
        description: "Pick, reject and rate a folder's photos.",
        arguments: &[("folder", "folder path; default the open folder", false), ("keep", "roughly how many to keep", false)],
        text: cull,
    },
    Template {
        name: "match_look",
        title: "Match a look",
        description: "Save one photo's look as a profile and fit it to other photos.",
        arguments: &[("source", "photo id whose look to copy", false), ("targets", "photo ids, comma separated; default the rest", false)],
        text: match_look,
    },
];

pub fn list() -> Vec<Prompt> {
    TEMPLATES
        .iter()
        .map(|t| {
            let arguments = t
                .arguments
                .iter()
                .map(|&(name, description, required)| {
                    let mut argument = PromptArgument::new(name);
                    argument.description = Some(description.into());
                    argument.required = Some(required);
                    argument
                })
                .collect();
            let mut prompt = Prompt::new(t.name, Some(t.description), Some(arguments));
            prompt.title = Some(t.title.into());
            prompt
        })
        .collect()
}

pub fn get(name: &str, args: &Map<String, Value>) -> Option<GetPromptResult> {
    let t = TEMPLATES.iter().find(|t| t.name == name)?;
    Some(GetPromptResult::new(vec![PromptMessage::new_text(Role::User, (t.text)(args))]).with_description(t.description))
}
