use crate::ctx::explorer_path;
use std::path::{Path, PathBuf};

/// Explorer passes a shared folder as `UNC\server\share\path`, which is why the
/// right-click menu refused every folder on a network drive.
#[test]
fn a_unc_path_from_explorer_is_rewritten_to_a_real_one() {
    assert_eq!(
        explorer_path(Path::new(r"UNC\Kz370s\g\Downloads")),
        PathBuf::from(r"\\Kz370s\g\Downloads")
    );
    assert_eq!(
        explorer_path(Path::new("UNC/Kz370s/g/Downloads")),
        PathBuf::from(r"\\Kz370s\g\Downloads")
    );
}

#[test]
fn ordinary_paths_are_left_alone() {
    for p in [
        r"C:\Users\you\Sites\shop",
        r"\\server\share\folder",
        r"\\?\C:\very\long\path",
        r"relative\folder",
        r"\\?\UNC\server\share\folder",
    ] {
        assert_eq!(explorer_path(Path::new(p)), PathBuf::from(p), "{p}");
    }
    // A local folder whose name merely starts with the letters is not a share.
    assert_eq!(
        explorer_path(Path::new(r"C:\UNC\folder")),
        PathBuf::from(r"C:\UNC\folder")
    );
}

mod docker_cli {
    use crate::args::{Cli, Cmd, ComposeCmd, DockerCmd};
    use clap::Parser;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        let mut all = vec!["olsc"];
        all.extend_from_slice(args);
        Cli::try_parse_from(all)
    }

    fn docker(args: &[&str]) -> DockerCmd {
        let mut all = vec!["docker"];
        all.extend_from_slice(args);
        match parse(&all).unwrap().cmd {
            Cmd::Docker(d) => d,
            _ => panic!("not a docker command"),
        }
    }

    #[test]
    fn dockerize_parses_and_defaults_to_a_plan_only() {
        match docker(&["dockerize", "shop"]) {
            DockerCmd::Dockerize {
                project,
                create,
                services,
                replace_compose,
                ..
            } => {
                assert_eq!(project, "shop");
                assert!(!create && !replace_compose && services.is_none());
            }
            _ => panic!("not dockerize"),
        }
        match docker(&[
            "dockerize",
            "shop",
            "--mode",
            "prod",
            "--services",
            "mariadb,redis",
            "--create",
        ]) {
            DockerCmd::Dockerize {
                services, create, ..
            } => {
                assert_eq!(services.unwrap(), vec!["mariadb", "redis"]);
                assert!(create);
            }
            _ => panic!("not dockerize"),
        }
        // Replacing the compose file only makes sense when writing.
        assert!(parse(&["docker", "dockerize", "shop", "--replace-compose"]).is_err());
        assert!(parse(&["docker", "dockerize", "shop", "--mode", "staging"]).is_err());
    }

    #[test]
    fn every_form_parses() {
        assert!(matches!(docker(&["status"]), DockerCmd::Status));
        assert!(matches!(docker(&["ps"]), DockerCmd::Ps { all: false }));
        assert!(matches!(
            docker(&["ps", "--all"]),
            DockerCmd::Ps { all: true }
        ));
        for a in ["start", "stop", "restart", "pause", "unpause"] {
            assert!(parse(&["docker", a, "web"]).is_ok(), "{a}");
        }
        assert!(matches!(
            docker(&["logs", "web", "-n", "50"]),
            DockerCmd::Logs { tail: 50, .. }
        ));
        assert!(matches!(
            docker(&["logs", "web"]),
            DockerCmd::Logs { tail: 200, .. }
        ));
        assert!(matches!(docker(&["images"]), DockerCmd::Images));
        assert!(matches!(docker(&["volumes"]), DockerCmd::Volumes));
        assert!(matches!(
            docker(&["df", "--fresh"]),
            DockerCmd::Df { fresh: true }
        ));
        assert!(matches!(
            docker(&["pull", "redis:7"]),
            DockerCmd::Pull { .. }
        ));
        assert!(matches!(
            docker(&["compose", "list"]),
            DockerCmd::Compose(ComposeCmd::List)
        ));
        for a in [
            "up", "down", "stop", "start", "restart", "pull", "build", "watch",
        ] {
            assert!(
                parse(&["docker", "compose", a, "compose.yaml", "--service", "db"]).is_ok(),
                "{a}"
            );
        }
        assert!(matches!(
            docker(&["compose", "check", "compose.yaml"]),
            DockerCmd::Compose(ComposeCmd::Check { .. })
        ));
        assert!(matches!(
            docker(&[
                "prune",
                "--dry-run",
                "--stopped",
                "--dangling",
                "--unused-images",
                "--build-cache"
            ]),
            DockerCmd::Prune {
                stopped: true,
                dangling: true,
                unused_images: true,
                build_cache: true,
                ..
            }
        ));
    }

    #[test]
    fn prune_without_dry_run_is_rejected() {
        assert!(parse(&["docker", "prune"]).is_err());
        assert!(parse(&["docker", "prune", "--stopped"]).is_err());
    }

    #[test]
    fn destructive_commands_do_not_exist() {
        for bad in [
            &["docker", "rm", "web"][..],
            &["docker", "volume", "rm", "data"],
            &["docker", "recreate", "web"],
            &["docker", "exec", "web", "sh"],
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }
}

mod db_branch_cli {
    use crate::args::{BranchCmd, Cli, Cmd, DbCmd};
    use clap::Parser;

    #[test]
    fn db_branch_commands_parse() {
        let cli =
            Cli::try_parse_from(["olsc", "db", "branch", "on", "shop", "--from-current"]).unwrap();
        match cli.cmd {
            Cmd::Db(DbCmd::Branch(BranchCmd::On {
                project,
                from_current,
            })) => {
                assert_eq!(project, "shop");
                assert!(from_current);
            }
            _ => panic!("not db branch on"),
        }
        assert!(Cli::try_parse_from(["olsc", "db", "branch", "status", "shop"]).is_ok());
        assert!(Cli::try_parse_from(["olsc", "db", "branch", "off", "shop"]).is_ok());
    }
}

mod db_pull_cli {
    use crate::args::{Cli, Cmd, DbCmd};
    use clap::Parser;

    #[test]
    fn db_pull_parses() {
        let cli = Cli::try_parse_from([
            "olsc",
            "db",
            "pull",
            "shop",
            "staging",
            "--anonymize",
            "--new",
        ])
        .unwrap();
        match cli.cmd {
            Cmd::Db(DbCmd::Pull {
                project,
                remote,
                anonymize,
                new,
                yes_real_data,
            }) => {
                assert_eq!((project.as_str(), remote.as_str()), ("shop", "staging"));
                assert!(anonymize && new && !yes_real_data);
            }
            _ => panic!("not db pull"),
        }
        assert!(Cli::try_parse_from(["olsc", "db", "remotes", "shop"]).is_ok());
    }
}

mod security_cli {
    use crate::args::{Cli, Cmd, ProjectCmd, RuntimeCmd};
    use clap::Parser;

    #[test]
    fn audit_and_eol_parse() {
        match Cli::try_parse_from(["olsc", "project", "audit", "--all"])
            .unwrap()
            .cmd
        {
            Cmd::Project(ProjectCmd::Audit { project, all }) => assert!(project.is_none() && all),
            _ => panic!("not project audit"),
        }
        assert!(matches!(
            Cli::try_parse_from(["olsc", "runtime", "eol"]).unwrap().cmd,
            Cmd::Runtime(RuntimeCmd::Eol)
        ));
    }
}
