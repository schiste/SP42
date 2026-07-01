//! Private presentation helpers shared by primitive modules.

pub(super) fn class_names(names: &[&str]) -> String {
    let mut class_name = String::new();
    for name in names {
        push_class(&mut class_name, name);
    }
    class_name
}

pub(super) fn push_class(class_name: &mut String, name: &str) {
    if name.is_empty() {
        return;
    }
    if !class_name.is_empty() {
        class_name.push(' ');
    }
    class_name.push_str(name);
}
