use clap::Subcommand;

use crate::error::GhostError;
use crate::projects;

#[derive(Debug, Subcommand)]
pub enum ProjectCommand {
    /// List all projects
    List {
        /// Filter by status (active, paused, completed, all)
        #[arg(long, default_value = "active")]
        status: String,
    },
    /// Initialize a new project
    Init {
        title: String,
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
    },
    /// Show project details and task summary
    Show { slug: String },
    /// Update project status
    Status {
        slug: String,
        /// New status (active, paused, completed)
        new_status: String,
    },
    /// Archive a project
    Archive { slug: String },
    /// Manage project tasks
    Task {
        #[command(subcommand)]
        command: TaskCommand,
    },
    /// Append an entry to the project log
    Log { slug: String, entry: String },
}

#[derive(Debug, Subcommand)]
pub enum TaskCommand {
    /// List tasks in a project
    List {
        slug: String,
        /// Filter by status (todo, in_progress, done, blocked)
        #[arg(long)]
        status: Option<String>,
    },
    /// Create a new task
    Create {
        slug: String,
        title: String,
        #[arg(long, value_delimiter = ',')]
        blocked_by: Vec<String>,
        #[arg(long)]
        body: Option<String>,
    },
    /// Show full task details
    Show { slug: String, task_slug: String },
    /// Update task status
    Status {
        slug: String,
        task_slug: String,
        /// New status (todo, in_progress, done, blocked)
        new_status: String,
    },
    /// Archive a task (or all done tasks if no task specified)
    Archive {
        slug: String,
        task_slug: Option<String>,
    },
}

#[tracing::instrument(skip_all)]
pub async fn execute(command: ProjectCommand) -> Result<(), GhostError> {
    let config = crate::config::load()?;
    crate::config_workspace::bootstrap_workspace(&config)?;

    match command {
        ProjectCommand::List { status } => cmd_list(&config.workspace, &status),
        ProjectCommand::Init { title, tags } => cmd_init(&config.workspace, &title, &tags),
        ProjectCommand::Show { slug } => cmd_show(&config.workspace, &slug),
        ProjectCommand::Status { slug, new_status } => {
            cmd_status(&config.workspace, &slug, &new_status)
        }
        ProjectCommand::Archive { slug } => cmd_archive(&config.workspace, &slug),
        ProjectCommand::Task { command } => execute_task(command, &config.workspace),
        ProjectCommand::Log { slug, entry } => cmd_log(&config.workspace, &slug, &entry),
    }
}

fn execute_task(command: TaskCommand, workspace: &std::path::Path) -> Result<(), GhostError> {
    match command {
        TaskCommand::List { slug, status } => cmd_task_list(workspace, &slug, status.as_deref()),
        TaskCommand::Create {
            slug,
            title,
            blocked_by,
            body,
        } => cmd_task_create(workspace, &slug, &title, &blocked_by, body.as_deref()),
        TaskCommand::Show { slug, task_slug } => cmd_task_show(workspace, &slug, &task_slug),
        TaskCommand::Status {
            slug,
            task_slug,
            new_status,
        } => cmd_task_status(workspace, &slug, &task_slug, &new_status),
        TaskCommand::Archive { slug, task_slug } => {
            cmd_task_archive(workspace, &slug, task_slug.as_deref())
        }
    }
}

// ---------------------------------------------------------------------------
// Project subcommands
// ---------------------------------------------------------------------------

fn cmd_list(workspace: &std::path::Path, status_filter: &str) -> Result<(), GhostError> {
    let all = projects::list_projects(workspace)?;

    let filtered: Vec<_> = if status_filter == "all" {
        all
    } else {
        let target: projects::ProjectStatus = status_filter
            .parse()
            .map_err(|e: String| projects::ProjectError::InvalidFrontMatter { reason: e })?;
        all.into_iter()
            .filter(|(_, p)| p.front.status == target)
            .collect()
    };

    if filtered.is_empty() {
        println!("No projects found.");
        return Ok(());
    }

    for (slug, project) in &filtered {
        let (done, total) = projects::task_summary(workspace, slug).unwrap_or((0, 0));
        println!(
            "  {:<24} {:<12} {:<30} {}/{} tasks done",
            slug, project.front.status, project.front.title, done, total
        );
    }

    Ok(())
}

fn cmd_init(workspace: &std::path::Path, title: &str, tags: &[String]) -> Result<(), GhostError> {
    let (slug, path) = projects::init_project(workspace, title, tags)?;
    println!("Created project: {slug}");
    println!("  {}", path.display());
    Ok(())
}

fn cmd_show(workspace: &std::path::Path, slug: &str) -> Result<(), GhostError> {
    let project = projects::read_project(workspace, slug)?;
    let (done, total) = projects::task_summary(workspace, slug).unwrap_or((0, 0));

    println!("# {}", project.front.title);
    println!("Status: {}", project.front.status);
    println!("Created: {}", project.front.created);
    if !project.front.tags.is_empty() {
        println!("Tags: {}", project.front.tags.join(", "));
    }
    if !project.body.trim().is_empty() {
        println!("\n{}", project.body.trim());
    }

    println!("\n## Tasks ({done}/{total} done)");

    let tasks = projects::list_tasks(workspace, slug)?;
    let priority = projects::read_priority(workspace, slug)?;

    // Show tasks in priority order, then unprioritized ones
    let mut shown = std::collections::HashSet::new();
    for task_slug in &priority {
        if let Some((_, task)) = tasks.iter().find(|(s, _)| s == task_slug) {
            println!(
                "  [{:<12}] {:<24} {}",
                task.front.status, task_slug, task.front.title
            );
            shown.insert(task_slug.as_str());
        }
    }
    for (task_slug, task) in &tasks {
        if !shown.contains(task_slug.as_str()) {
            println!(
                "  [{:<12}] {:<24} {}",
                task.front.status, task_slug, task.front.title
            );
        }
    }

    Ok(())
}

fn cmd_status(workspace: &std::path::Path, slug: &str, new_status: &str) -> Result<(), GhostError> {
    let mut project = projects::read_project(workspace, slug)?;
    let status: projects::ProjectStatus = new_status
        .parse()
        .map_err(|e: String| projects::ProjectError::InvalidFrontMatter { reason: e })?;
    project.front.status = status;
    projects::write_project(workspace, slug, &project.front, &project.body)?;
    println!("Project '{slug}' status -> {status}");
    Ok(())
}

fn cmd_archive(workspace: &std::path::Path, slug: &str) -> Result<(), GhostError> {
    projects::archive_project(workspace, slug)?;
    println!("Archived project: {slug}");
    Ok(())
}

fn cmd_log(workspace: &std::path::Path, slug: &str, entry: &str) -> Result<(), GhostError> {
    projects::append_log(workspace, slug, entry)?;
    println!("Log entry added to project: {slug}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Task subcommands
// ---------------------------------------------------------------------------

fn cmd_task_list(
    workspace: &std::path::Path,
    slug: &str,
    status_filter: Option<&str>,
) -> Result<(), GhostError> {
    let tasks = projects::list_tasks(workspace, slug)?;

    let filtered: Vec<_> = if let Some(filter) = status_filter {
        let target: projects::TaskStatus = filter
            .parse()
            .map_err(|e: String| projects::ProjectError::InvalidFrontMatter { reason: e })?;
        tasks
            .into_iter()
            .filter(|(_, t)| t.front.status == target)
            .collect()
    } else {
        tasks
    };

    if filtered.is_empty() {
        println!("No tasks found.");
        return Ok(());
    }

    let priority = projects::read_priority(workspace, slug)?;
    let mut shown = std::collections::HashSet::new();

    // Priority-ordered first
    for task_slug in &priority {
        if let Some((_, task)) = filtered.iter().find(|(s, _)| s == task_slug) {
            println!(
                "  [{:<12}] {:<24} {}",
                task.front.status, task_slug, task.front.title
            );
            shown.insert(task_slug.as_str());
        }
    }
    // Then unprioritized
    for (task_slug, task) in &filtered {
        if !shown.contains(task_slug.as_str()) {
            println!(
                "  [{:<12}] {:<24} {}",
                task.front.status, task_slug, task.front.title
            );
        }
    }

    Ok(())
}

fn cmd_task_create(
    workspace: &std::path::Path,
    slug: &str,
    title: &str,
    blocked_by: &[String],
    body: Option<&str>,
) -> Result<(), GhostError> {
    let (task_slug, _) =
        projects::create_task(workspace, slug, title, blocked_by, body.unwrap_or(""))?;
    println!("Created task: {task_slug} (project: {slug})");
    Ok(())
}

fn cmd_task_show(
    workspace: &std::path::Path,
    slug: &str,
    task_slug: &str,
) -> Result<(), GhostError> {
    let task = projects::read_task(workspace, slug, task_slug)?;

    println!("# {}", task.front.title);
    println!("Status: {}", task.front.status);
    println!("Created: {}", task.front.created);
    if !task.front.blocked_by.is_empty() {
        println!("Blocked by: {}", task.front.blocked_by.join(", "));
    }
    if !task.body.trim().is_empty() {
        println!("\n{}", task.body.trim());
    }

    Ok(())
}

fn cmd_task_status(
    workspace: &std::path::Path,
    slug: &str,
    task_slug: &str,
    new_status: &str,
) -> Result<(), GhostError> {
    let mut task = projects::read_task(workspace, slug, task_slug)?;
    let status: projects::TaskStatus = new_status
        .parse()
        .map_err(|e: String| projects::ProjectError::InvalidFrontMatter { reason: e })?;
    task.front.status = status;
    projects::write_task(workspace, slug, task_slug, &task.front, &task.body)?;
    println!("Task '{task_slug}' status -> {status}");
    Ok(())
}

fn cmd_task_archive(
    workspace: &std::path::Path,
    slug: &str,
    task_slug: Option<&str>,
) -> Result<(), GhostError> {
    if let Some(task_slug) = task_slug {
        projects::archive_task(workspace, slug, task_slug)?;
        println!("Archived task: {task_slug}");
    } else {
        let count = projects::archive_done_tasks(workspace, slug)?;
        println!("Archived {count} done task(s)");
    }
    Ok(())
}
