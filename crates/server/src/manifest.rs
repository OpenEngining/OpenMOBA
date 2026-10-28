use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

pub fn manifest(root: &Path) -> String {
    let mut lines = Vec::new();
    collect(root, root, &mut lines);
    lines.sort();
    lines.concat()
}

fn collect(root: &Path, folder: &Path, lines: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for path in entries.flatten().map(|entry| entry.path()) {
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            collect(root, &path, lines);
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let name = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let size = metadata.len();
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |age| age.as_secs());
        lines.push(format!("{name}\t{size}\t{modified:x}{size:x}\n"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_lists_every_file_with_its_size() {
        let root = std::env::temp_dir().join(format!("lll-manifest-{}", std::process::id()));
        fs::create_dir_all(root.join("champions")).unwrap();
        fs::write(root.join("data.bin"), [0; 3]).unwrap();
        fs::write(root.join("champions/emperor.bin"), [0; 10]).unwrap();
        let listed = manifest(&root);
        fs::remove_dir_all(&root).unwrap();
        let entries: Vec<Vec<&str>> = listed
            .lines()
            .map(|line| line.split('\t').collect())
            .collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0][..2], ["champions/emperor.bin", "10"]);
        assert_eq!(entries[1][..2], ["data.bin", "3"]);
        assert!(entries[0][2].ends_with('a'));
    }
}
