//! What Craft AI does inside EffectCraft. Craft AI is one assistant across the ArtCraft family
//! — the model, the runtime and the shared folder belong to the family — but its *work* is the
//! app's own: the system prompt tells the model which app it is inside and what that app does,
//! and every ready-made task is framed for that app's world, so the answer that comes back is
//! about compositions and keyframes and expressions, not generic advice.
//!
//! The labels and hints are catalog keys: the panel shows them translated.

/// The app Craft AI is embedded in, as the shared folder's records name it.
pub const APP: &str = "EffectCraft";

/// One ready-made task of the Assistant tab.
pub struct Task {
    /// The label the picker shows (a catalog key).
    pub label: &'static str,
    /// The input field's hint (a catalog key).
    pub hint: &'static str,
}

/// The tasks, in the picker's order.
pub const TASKS: &[Task] = &[
    Task { label: "Ask anything", hint: "Ask in any language" },
    Task { label: "Write an expression", hint: "What it must compute, and on which property" },
    Task { label: "Effect recipe", hint: "The look you are after, and what the footage is" },
    Task { label: "Keyframe and easing plan", hint: "The motion, and where it must be slow or fast" },
    Task { label: "Precompose advice", hint: "The layers and what you want to reuse or isolate" },
    Task { label: "Render settings", hint: "Where it goes (web, broadcast, archive), and the length" },
];

/// Tell the model where it is and how to answer: briefly, practically, in the language the user
/// wrote in, about this app's own world — the words of its interface, the files it opens and
/// writes. The last line is the honesty clause: a local model can be wrong, and it is told to
/// say so rather than invent.
pub fn system_prompt() -> String {
    "You are Craft AI, the assistant built into EffectCraft, a desktop motion-graphics and \
     compositing app. EffectCraft's world, in its own words: compositions, layers and their \
     transforms; masks and mattes; effects and their properties; keyframes and the graph \
     editor; expressions — the little JavaScript-like snippets that drive a property, written \
     in the expression field; precomps and nesting; shape layers and text animators; the \
     tracker, wiggler and paint; motion blur and frame blending; the render queue and output \
     module settings. Answer in the language the user wrote in, Arabic included. Be brief and \
     practical: name the panel or the property, give the steps as a short list, say what to \
     watch out for, and when a question is not about EffectCraft or motion graphics, answer it \
     anyway, briefly. Expressions you write must be syntactically valid for the expression \
     field. Never invent menu entries, panel names or effect names: if you are not sure \
     EffectCraft has a feature, say so plainly."
        .to_string()
}

/// Frame the user's input for the task they picked.
pub fn user_prompt(task: usize, input: &str) -> String {
    let input = input.trim();
    match TASKS[task.min(TASKS.len() - 1)].label {
        "Write an expression" => format!(
            "Write an EffectCraft expression for this — the property it drives, the snippet itself, and one line on how it works: {input}"
        ),
        "Effect recipe" => format!(
            "Build an effect recipe in EffectCraft for this look — the effects in order, the properties to set, what to watch out for: {input}"
        ),
        "Keyframe and easing plan" => format!(
            "Plan the keyframes and easing in EffectCraft for this motion — where the keyframes sit, which easing, where the graph editor helps: {input}"
        ),
        "Precompose advice" => format!(
            "Advise on precomposing in EffectCraft here — what to group, what stays out, what it costs and what it buys: {input}"
        ),
        "Render settings" => format!(
            "Recommend render queue and output module settings in EffectCraft for where this is going — format, codec, resolution, frame rate — in a short list: {input}"
        ),
        _ => input.to_string(),
    }
}
