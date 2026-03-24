use std::path::PathBuf;

use clap::Subcommand;

use crate::error::GhostError;
use crate::skills;

#[derive(Debug, Subcommand)]
pub enum SkillsCommand {
    /// List skills available in chat.
    List,
    /// List skills available to the coding agent in a directory.
    Coding {
        /// Working directory for the coding session.
        dir: PathBuf,
    },
    /// Show the full content of a skill (frontmatter stripped).
    Show {
        /// Skill name (as shown in `ghost skills list`).
        name: String,
    },
}

pub async fn execute(command: SkillsCommand) -> Result<(), GhostError> {
    let config = crate::config::load()?;

    match command {
        SkillsCommand::List => {
            let all = skills::discover_skills(&config.workspace);
            let chat_skills: Vec<_> = all
                .into_iter()
                .filter(|s| s.available.as_deref() != Some("coding"))
                .collect();

            if chat_skills.is_empty() {
                println!("No skills found in {}/skills/", config.workspace.display());
                return Ok(());
            }

            print_skills(&chat_skills, &[]);
        }
        SkillsCommand::Coding { dir } => {
            let workspace_skills = skills::discover_skills(&config.workspace);

            let repo_skills_dir = dir.join(".agents").join("skills");
            let repo_skills = if repo_skills_dir.is_dir() {
                skills::discover_repo_skills(&repo_skills_dir)
            } else {
                Vec::new()
            };

            if workspace_skills.is_empty() && repo_skills.is_empty() {
                println!("No skills found.");
                return Ok(());
            }

            let repo_names: Vec<String> = repo_skills.iter().map(|s| s.name.clone()).collect();
            let mut all = workspace_skills;
            all.extend(repo_skills);
            all.sort_by(|a, b| a.name.cmp(&b.name));

            print_skills(&all, &repo_names);
        }
        SkillsCommand::Show { name } => {
            let all = skills::discover_skills(&config.workspace);
            let skill = all.iter().find(|s| s.name == name);

            match skill {
                Some(s) => {
                    let content = std::fs::read_to_string(&s.path)?;
                    print!("{}", skills::strip_frontmatter_body(&content));
                }
                None => {
                    eprintln!("Skill '{name}' not found.");
                    std::process::exit(1);
                }
            }
        }
    }

    Ok(())
}

fn print_skills(skills: &[skills::Skill], local_names: &[String]) {
    let max_name = skills.iter().map(|s| s.name.len()).max().unwrap_or(0);

    for skill in skills {
        let tag = if local_names.contains(&skill.name) {
            "  (local)"
        } else {
            ""
        };
        println!(
            "  {:<width$}  {}{}",
            skill.name,
            skill.description,
            tag,
            width = max_name,
        );
    }
}
