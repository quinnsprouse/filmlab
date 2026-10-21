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
        "Grade {photo} in Filmlab.{look} Read guide workflow first. Look at it with look preview, then work one idea at a \
         time: exposure and white balance, then the film, then local fixes with masks, editing values and film together \
         when you know both. Each edit answers with its effect, so read that rather than measuring again; use look trials \
         when choosing between options, and fit to land a number. Check faces, skies and highlights at full detail with \
         a region. Finish with look compare from \"before\" (image pair) and a short note of what you changed and why."
    )
}

fn cull(args: &Map<String, Value>) -> String {
    let folder = arg(args, "folder").map_or("the folder open in Filmlab".to_string(), |f| format!("the folder {f} (photos action folder)"));
    let keep = arg(args, "keep").map_or(String::new(), |k| format!(" Keep about {k}."));
    format!(
        "Cull {folder}.{keep} Look at the photos a dozen at a time in contact sheets (look preview with a list of ids); \
         look at one alone, with a region at full detail, only to check focus or eyes. Flag the best as pick and the \
         failures (missed focus, blur, closed eyes, duplicates) as reject, and rate keepers 1-5, with photos action flags. \
         Put near duplicates in a sheet together before choosing. End with photos action list (filter picks) and one line \
         per pick saying why."
    )
}

fn match_look(args: &Map<String, Value>) -> String {
    let source = arg(args, "source").unwrap_or("the photo open in Filmlab");
    let targets = arg(args, "targets").map_or("the other photos in the folder".to_string(), |t| format!("photos {t}"));
    format!(
        "Carry the look of {source} to {targets} with edit paste (photo: the targets); it keeps each photo's own exposure \
         and white balance. Use match only for photos of the same scene and light. Don't save a profile unless the user \
         asks for one. Check them together in one contact sheet with the source, fix any that stand out, and report what \
         differed."
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
        description: "Paste one photo's look onto other photos and fit each to it.",
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
