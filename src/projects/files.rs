use std::path::{Path, PathBuf};

use crate::knowledge::slug_from_title;

use super::error::ProjectError;
use super::parser::{
    parse_priority_list, parse_project, parse_task, serialize_priority_list, serialize_project,
    serialize_task,
};
use super::types::{
    ParsedProject, ParsedTask, ProjectFrontMatter, ProjectStatus, TaskFrontMatter, TaskStatus,
};

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

#[must_use]
pub fn projects_dir(workspace: &Path) -> PathBuf {
    workspace.join("projects")
}

#[must_use]
pub fn project_dir(workspace: &Path, slug: &str) -> PathBuf {
    projects_dir(workspace).join(slug)
}

#[must_use]
fn task_file(workspace: &Path, project_slug: &str, task_slug: &str) -> PathBuf {
    project_dir(workspace, project_slug)
        .join("tasks")
        .join(format!("{task_slug}.md"))
}

#[must_use]
fn archive_task_file(workspace: &Path, project_slug: &str, task_slug: &str) -> PathBuf {
    project_dir(workspace, project_slug)
        .join("tasks")
        .join(".archive")
        .join(format!("{task_slug}.md"))
}

#[must_use]
fn priority_file(workspace: &Path, project_slug: &str) -> PathBuf {
    project_dir(workspace, project_slug)
        .join("tasks")
        .join("PRIORITY.md")
}

// ---------------------------------------------------------------------------
// Project operations
// ---------------------------------------------------------------------------

/// Initialize a new project. Returns `(slug, project_dir_path)`.
pub fn init_project(
    workspace: &Path,
    title: &str,
    tags: &[String],
) -> Result<(String, PathBuf), ProjectError> {
    let slug = slug_from_title(title);
    let dir = project_dir(workspace, &slug);

    if dir.exists() {
        return Err(ProjectError::InvalidFrontMatter {
            reason: format!("project directory already exists: {}", dir.display()),
        });
    }

    // Create directory structure
    for sub in ["tasks", "tasks/.archive", "notes", "references"] {
        let path = dir.join(sub);
        std::fs::create_dir_all(&path).map_err(|source| ProjectError::Io {
            path: path.clone(),
            source,
        })?;
    }

    let created = chrono::Local::now().format("%Y-%m-%d").to_string();
    let front = ProjectFrontMatter {
        title: title.to_string(),
        status: ProjectStatus::Active,
        created,
        tags: tags.to_vec(),
    };

    let index = dir.join("index.md");
    let content = serialize_project(&front, "")?;
    std::fs::write(&index, &content).map_err(|source| ProjectError::Io {
        path: index,
        source,
    })?;

    // Empty priority file
    let prio = priority_file(workspace, &slug);
    let prio_content = serialize_priority_list(&[]);
    std::fs::write(&prio, prio_content)
        .map_err(|source| ProjectError::Io { path: prio, source })?;

    // Empty log file
    let log = dir.join("log.md");
    std::fs::write(&log, "").map_err(|source| ProjectError::Io { path: log, source })?;

    Ok((slug, dir))
}

/// List all projects. Returns `(slug, ParsedProject)` pairs, sorted by slug.
pub fn list_projects(workspace: &Path) -> Result<Vec<(String, ParsedProject)>, ProjectError> {
    let base = projects_dir(workspace);
    if !base.exists() {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    let entries = std::fs::read_dir(&base).map_err(|source| ProjectError::Io {
        path: base.clone(),
        source,
    })?;

    for entry in entries {
        let entry = entry.map_err(|source| ProjectError::Io {
            path: base.clone(),
            source,
        })?;

        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || !entry.path().is_dir() {
            continue;
        }

        let index = entry.path().join("index.md");
        if !index.exists() {
            continue;
        }

        let content = std::fs::read_to_string(&index).map_err(|source| ProjectError::Io {
            path: index,
            source,
        })?;

        match parse_project(&content) {
            Ok(project) => results.push((name, project)),
            Err(_) => continue,
        }
    }

    results.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(results)
}

pub fn read_project(workspace: &Path, slug: &str) -> Result<ParsedProject, ProjectError> {
    let index = project_dir(workspace, slug).join("index.md");
    if !index.exists() {
        return Err(ProjectError::ProjectNotFound {
            slug: slug.to_string(),
        });
    }

    let content = std::fs::read_to_string(&index).map_err(|source| ProjectError::Io {
        path: index,
        source,
    })?;
    parse_project(&content)
}

pub fn write_project(
    workspace: &Path,
    slug: &str,
    front: &ProjectFrontMatter,
    body: &str,
) -> Result<PathBuf, ProjectError> {
    let index = project_dir(workspace, slug).join("index.md");
    let content = serialize_project(front, body)?;
    std::fs::write(&index, &content).map_err(|source| ProjectError::Io {
        path: index.clone(),
        source,
    })?;
    Ok(index)
}

/// Archive a project by moving its directory to `projects/.archive/`.
pub fn archive_project(workspace: &Path, slug: &str) -> Result<(), ProjectError> {
    let src = project_dir(workspace, slug);
    if !src.exists() {
        return Err(ProjectError::ProjectNotFound {
            slug: slug.to_string(),
        });
    }

    let archive_dir = projects_dir(workspace).join(".archive");
    std::fs::create_dir_all(&archive_dir).map_err(|source| ProjectError::Io {
        path: archive_dir.clone(),
        source,
    })?;

    let dest = archive_dir.join(slug);
    std::fs::rename(&src, &dest).map_err(|source| ProjectError::Io { path: src, source })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Task operations
// ---------------------------------------------------------------------------

/// List all tasks in a project. Returns `(task_slug, ParsedTask)` pairs.
/// Tasks in `.archive/` and `PRIORITY.md` are excluded.
pub fn list_tasks(
    workspace: &Path,
    project_slug: &str,
) -> Result<Vec<(String, ParsedTask)>, ProjectError> {
    let dir = project_dir(workspace, project_slug);
    if !dir.exists() {
        return Err(ProjectError::ProjectNotFound {
            slug: project_slug.to_string(),
        });
    }

    let tasks_dir = dir.join("tasks");
    if !tasks_dir.exists() {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    let entries = std::fs::read_dir(&tasks_dir).map_err(|source| ProjectError::Io {
        path: tasks_dir.clone(),
        source,
    })?;

    for entry in entries {
        let entry = entry.map_err(|source| ProjectError::Io {
            path: tasks_dir.clone(),
            source,
        })?;

        let name = entry.file_name().to_string_lossy().to_string();
        // Skip directories (.archive), non-md files, and PRIORITY.md
        if !name.ends_with(".md") || name == "PRIORITY.md" || entry.path().is_dir() {
            continue;
        }

        let slug = name.trim_end_matches(".md").to_string();
        let content = std::fs::read_to_string(entry.path()).map_err(|source| ProjectError::Io {
            path: entry.path(),
            source,
        })?;

        match parse_task(&content) {
            Ok(task) => results.push((slug, task)),
            Err(_) => continue,
        }
    }

    results.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(results)
}

pub fn read_task(
    workspace: &Path,
    project_slug: &str,
    task_slug: &str,
) -> Result<ParsedTask, ProjectError> {
    let path = task_file(workspace, project_slug, task_slug);
    if !path.exists() {
        return Err(ProjectError::TaskNotFound {
            project: project_slug.to_string(),
            task: task_slug.to_string(),
        });
    }

    let content =
        std::fs::read_to_string(&path).map_err(|source| ProjectError::Io { path, source })?;
    parse_task(&content)
}

pub fn write_task(
    workspace: &Path,
    project_slug: &str,
    task_slug: &str,
    front: &TaskFrontMatter,
    body: &str,
) -> Result<PathBuf, ProjectError> {
    let path = task_file(workspace, project_slug, task_slug);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ProjectError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let content = serialize_task(front, body)?;
    std::fs::write(&path, &content).map_err(|source| ProjectError::Io {
        path: path.clone(),
        source,
    })?;
    Ok(path)
}

/// Create a new task: writes the file and appends to PRIORITY.md.
pub fn create_task(
    workspace: &Path,
    project_slug: &str,
    title: &str,
    blocked_by: &[String],
    body: &str,
) -> Result<(String, PathBuf), ProjectError> {
    let dir = project_dir(workspace, project_slug);
    if !dir.exists() {
        return Err(ProjectError::ProjectNotFound {
            slug: project_slug.to_string(),
        });
    }

    let task_slug = slug_from_title(title);
    let status = if blocked_by.is_empty() {
        TaskStatus::Todo
    } else {
        TaskStatus::Blocked
    };
    let created = chrono::Local::now().format("%Y-%m-%d").to_string();

    let front = TaskFrontMatter {
        title: title.to_string(),
        status,
        blocked_by: blocked_by.to_vec(),
        created,
    };

    let path = write_task(workspace, project_slug, &task_slug, &front, body)?;

    // Append to priority list
    let mut priority = read_priority(workspace, project_slug)?;
    priority.push(task_slug.clone());
    write_priority(workspace, project_slug, &priority)?;

    Ok((task_slug, path))
}

/// Archive a single task (move to `.archive/` and remove from PRIORITY.md).
pub fn archive_task(
    workspace: &Path,
    project_slug: &str,
    task_slug: &str,
) -> Result<(), ProjectError> {
    let src = task_file(workspace, project_slug, task_slug);
    if !src.exists() {
        return Err(ProjectError::TaskNotFound {
            project: project_slug.to_string(),
            task: task_slug.to_string(),
        });
    }

    let dest = archive_task_file(workspace, project_slug, task_slug);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ProjectError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    std::fs::rename(&src, &dest).map_err(|source| ProjectError::Io { path: src, source })?;

    // Remove from priority list
    let priority = read_priority(workspace, project_slug)?;
    let filtered: Vec<String> = priority.into_iter().filter(|s| s != task_slug).collect();
    write_priority(workspace, project_slug, &filtered)?;

    Ok(())
}

/// Archive all tasks with `Done` status. Returns the count of archived tasks.
pub fn archive_done_tasks(workspace: &Path, project_slug: &str) -> Result<usize, ProjectError> {
    let tasks = list_tasks(workspace, project_slug)?;
    let done_slugs: Vec<String> = tasks
        .iter()
        .filter(|(_, t)| t.front.status == TaskStatus::Done)
        .map(|(slug, _)| slug.clone())
        .collect();

    let count = done_slugs.len();
    for slug in &done_slugs {
        archive_task(workspace, project_slug, slug)?;
    }

    Ok(count)
}

// ---------------------------------------------------------------------------
// Priority list
// ---------------------------------------------------------------------------

pub fn read_priority(workspace: &Path, project_slug: &str) -> Result<Vec<String>, ProjectError> {
    let path = priority_file(workspace, project_slug);
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content =
        std::fs::read_to_string(&path).map_err(|source| ProjectError::Io { path, source })?;
    Ok(parse_priority_list(&content))
}

pub fn write_priority(
    workspace: &Path,
    project_slug: &str,
    slugs: &[String],
) -> Result<(), ProjectError> {
    let path = priority_file(workspace, project_slug);
    let content = serialize_priority_list(slugs);
    std::fs::write(&path, content).map_err(|source| ProjectError::Io { path, source })
}

// ---------------------------------------------------------------------------
// Log
// ---------------------------------------------------------------------------

pub fn append_log(workspace: &Path, project_slug: &str, entry: &str) -> Result<(), ProjectError> {
    let dir = project_dir(workspace, project_slug);
    if !dir.exists() {
        return Err(ProjectError::ProjectNotFound {
            slug: project_slug.to_string(),
        });
    }

    let path = dir.join("log.md");
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();

    let mut existing = std::fs::read_to_string(&path).unwrap_or_default();
    if !existing.is_empty() && !existing.ends_with('\n') {
        existing.push('\n');
    }
    existing.push_str(&format!("\n## {timestamp}\n\n{entry}\n"));

    std::fs::write(&path, existing).map_err(|source| ProjectError::Io { path, source })
}

// ---------------------------------------------------------------------------
// Task summary (for system prompt / CLI)
// ---------------------------------------------------------------------------

/// Count done vs total tasks for a project.
pub fn task_summary(workspace: &Path, project_slug: &str) -> Result<(usize, usize), ProjectError> {
    let tasks = list_tasks(workspace, project_slug)?;
    let done = tasks
        .iter()
        .filter(|(_, t)| t.front.status == TaskStatus::Done)
        .count();
    Ok((done, tasks.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_workspace() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("projects")).unwrap();
        dir
    }

    #[test]
    fn init_and_read_project() {
        let ws = setup_workspace();
        let (slug, path) = init_project(ws.path(), "Build Website", &["web".into()]).unwrap();

        assert_eq!(slug, "build_website");
        assert!(path.exists());
        assert!(path.join("index.md").exists());
        assert!(path.join("tasks").exists());
        assert!(path.join("tasks/.archive").exists());
        assert!(path.join("notes").exists());
        assert!(path.join("references").exists());
        assert!(path.join("log.md").exists());
        assert!(path.join("tasks/PRIORITY.md").exists());

        let project = read_project(ws.path(), &slug).unwrap();
        assert_eq!(project.front.title, "Build Website");
        assert_eq!(project.front.status, ProjectStatus::Active);
        assert_eq!(project.front.tags, vec!["web"]);
    }

    #[test]
    fn init_duplicate_project_errors() {
        let ws = setup_workspace();
        init_project(ws.path(), "Test", &[]).unwrap();
        let result = init_project(ws.path(), "Test", &[]);
        assert!(result.is_err());
    }

    #[test]
    fn list_projects_filters_hidden_and_non_dirs() {
        let ws = setup_workspace();
        init_project(ws.path(), "Alpha", &[]).unwrap();
        init_project(ws.path(), "Beta", &[]).unwrap();

        // Create a hidden dir and a non-dir file
        std::fs::create_dir_all(ws.path().join("projects/.archive")).unwrap();
        std::fs::write(ws.path().join("projects/stray.txt"), "junk").unwrap();

        let projects = list_projects(ws.path()).unwrap();
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].0, "alpha");
        assert_eq!(projects[1].0, "beta");
    }

    #[test]
    fn create_and_list_tasks() {
        let ws = setup_workspace();
        let (slug, _) = init_project(ws.path(), "Test", &[]).unwrap();

        create_task(ws.path(), &slug, "First Task", &[], "Do it.").unwrap();
        create_task(ws.path(), &slug, "Second Task", &[], "Do it too.").unwrap();

        let tasks = list_tasks(ws.path(), &slug).unwrap();
        assert_eq!(tasks.len(), 2);

        let priority = read_priority(ws.path(), &slug).unwrap();
        assert_eq!(priority, vec!["first_task", "second_task"]);
    }

    #[test]
    fn create_task_with_blocked_by() {
        let ws = setup_workspace();
        let (slug, _) = init_project(ws.path(), "Test", &[]).unwrap();

        create_task(ws.path(), &slug, "Setup", &[], "").unwrap();
        create_task(ws.path(), &slug, "Deploy", &["setup".into()], "").unwrap();

        let task = read_task(ws.path(), &slug, "deploy").unwrap();
        assert_eq!(task.front.status, TaskStatus::Blocked);
        assert_eq!(task.front.blocked_by, vec!["setup"]);
    }

    #[test]
    fn archive_task_moves_and_updates_priority() {
        let ws = setup_workspace();
        let (slug, _) = init_project(ws.path(), "Test", &[]).unwrap();
        create_task(ws.path(), &slug, "Done Task", &[], "").unwrap();

        archive_task(ws.path(), &slug, "done_task").unwrap();

        assert!(!task_file(ws.path(), &slug, "done_task").exists());
        assert!(archive_task_file(ws.path(), &slug, "done_task").exists());
        assert!(read_priority(ws.path(), &slug).unwrap().is_empty());
    }

    #[test]
    fn archive_done_tasks_bulk() {
        let ws = setup_workspace();
        let (slug, _) = init_project(ws.path(), "Test", &[]).unwrap();

        // Create two tasks, mark one done
        create_task(ws.path(), &slug, "Keep", &[], "").unwrap();
        let (done_slug, _) = create_task(ws.path(), &slug, "Finish", &[], "").unwrap();

        let mut task = read_task(ws.path(), &slug, &done_slug).unwrap();
        task.front.status = TaskStatus::Done;
        write_task(ws.path(), &slug, &done_slug, &task.front, &task.body).unwrap();

        let count = archive_done_tasks(ws.path(), &slug).unwrap();
        assert_eq!(count, 1);

        let remaining = list_tasks(ws.path(), &slug).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].0, "keep");
    }

    #[test]
    fn archive_project_moves_to_archive() {
        let ws = setup_workspace();
        let (slug, _) = init_project(ws.path(), "Old Project", &[]).unwrap();

        archive_project(ws.path(), &slug).unwrap();

        assert!(!project_dir(ws.path(), &slug).exists());
        assert!(
            ws.path()
                .join("projects/.archive")
                .join(&slug)
                .join("index.md")
                .exists()
        );
    }

    #[test]
    fn append_log_entries() {
        let ws = setup_workspace();
        let (slug, _) = init_project(ws.path(), "Test", &[]).unwrap();

        append_log(ws.path(), &slug, "Started work.").unwrap();
        append_log(ws.path(), &slug, "Made progress.").unwrap();

        let log = std::fs::read_to_string(project_dir(ws.path(), &slug).join("log.md")).unwrap();
        assert!(log.contains("Started work."));
        assert!(log.contains("Made progress."));
        // Two timestamp headers
        assert_eq!(log.matches("## 20").count(), 2);
    }

    #[test]
    fn task_summary_counts() {
        let ws = setup_workspace();
        let (slug, _) = init_project(ws.path(), "Test", &[]).unwrap();

        create_task(ws.path(), &slug, "A", &[], "").unwrap();
        let (b_slug, _) = create_task(ws.path(), &slug, "B", &[], "").unwrap();

        // Mark B as done
        let mut task = read_task(ws.path(), &slug, &b_slug).unwrap();
        task.front.status = TaskStatus::Done;
        write_task(ws.path(), &slug, &b_slug, &task.front, &task.body).unwrap();

        let (done, total) = task_summary(ws.path(), &slug).unwrap();
        assert_eq!(done, 1);
        assert_eq!(total, 2);
    }

    #[test]
    fn write_and_update_project() {
        let ws = setup_workspace();
        let (slug, _) = init_project(ws.path(), "Test", &[]).unwrap();

        let mut project = read_project(ws.path(), &slug).unwrap();
        project.front.status = ProjectStatus::Paused;
        write_project(ws.path(), &slug, &project.front, &project.body).unwrap();

        let updated = read_project(ws.path(), &slug).unwrap();
        assert_eq!(updated.front.status, ProjectStatus::Paused);
    }

    #[test]
    fn project_not_found() {
        let ws = setup_workspace();
        let result = read_project(ws.path(), "nonexistent");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("project not found")
        );
    }

    #[test]
    fn task_not_found() {
        let ws = setup_workspace();
        init_project(ws.path(), "Test", &[]).unwrap();
        let result = read_task(ws.path(), "test", "nonexistent");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("task not found"));
    }
}
