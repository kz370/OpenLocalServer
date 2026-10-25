use std::path::Path;

use crate::app::*;
use crate::domain::Domain;
use crate::error::CoreError;

impl Inner {
    pub fn wp_projects(&self) -> Vec<crate::wordpress::WpProject> {
        let projects = self.projects.lock().unwrap().list();
        let cfg = self.web_config();
        let domains = self.domains.lock().unwrap().list();
        projects
            .into_iter()
            .filter_map(|p| {
                let mut wp = crate::wordpress::describe(&p.id, &p.name, Path::new(&p.path))?;
                if let Some(d) = domains
                    .iter()
                    .filter(|d| d.project_id.as_deref() == Some(p.id.as_str()) && d.enabled)
                    .min_by_key(|d| d.hostname.clone())
                {
                    wp.hostname = Some(d.hostname.clone());
                    wp.url = Some(self.site_url(d, &cfg));
                }
                Some(wp)
            })
            .collect()
    }

    /// The site a WordPress project's sign-in link should point at: the hostname asked
    /// for when it belongs to this project, else the project's first enabled site.
    fn wp_site_for(
        &self,
        project: &crate::project::Project,
        hostname: Option<&str>,
    ) -> Result<Domain, CoreError> {
        let domains = self.domains.lock().unwrap().list();
        let mut mine: Vec<&Domain> = domains
            .iter()
            .filter(|d| d.project_id.as_deref() == Some(project.id.as_str()))
            .collect();
        mine.sort_by_key(|d| d.hostname.clone());
        let site = match hostname {
            Some(host) => mine.iter().copied().find(|d| d.hostname == host),
            None => mine.iter().copied().find(|d| d.enabled),
        };
        site.cloned().ok_or_else(|| match hostname {
            Some(host) => CoreError::failed_fix(
                format!("{host} is not a site of this project."),
                format!("{} has no domain called {host} behind it.", project.name),
                "Open the site's own menu to sign in to WordPress, or add a domain for this project first.",
            ),
            None => CoreError::failed_fix(
                format!("{} has no site to open yet.", project.name),
                "A WordPress sign-in link needs a URL, and this project has no domain pointing at it.",
                "Give the project a domain first (Add auto domain in its menu), then sign in.",
            ),
        })
    }

    /// Writes a one-time admin sign-in bridge and returns the link to open (§40's project
    /// menu). No password is read, asked for, or changed: the link carries a random token
    /// that WordPress itself exchanges for a session cookie, once, from this machine.
    pub fn wp_signin(
        &self,
        project_id: &str,
        hostname: Option<&str>,
        theme: Option<&str>,
    ) -> Result<crate::wordpress::WpSignIn, CoreError> {
        let project = self
            .projects
            .lock()
            .unwrap()
            .get(project_id)
            .ok_or_else(|| {
                CoreError::failed_fix(
                    "This project is not on the list any more.",
                    format!("No project is registered under the id {project_id}."),
                    "Reload the Sites page and pick the project again.",
                )
            })?;
        if !self.web.is_running() {
            return Err(CoreError::failed_fix(
                "The web server is not running.",
                "A WordPress sign-in link is opened in the browser, and the browser needs the site to answer first.",
                "Start the web server on the Web server page, then sign in again.",
            ));
        }
        let site = self.wp_site_for(&project, hostname)?;
        let cfg = self.web_config();
        let signin = crate::wordpress::issue(
            Path::new(&project.path),
            &self.site_url(&site, &cfg),
            None,
            theme,
        )?;
        // The URL carries the token, so it is deliberately not logged.
        tracing::info!(
            project = %project.name,
            hostname = %site.hostname,
            expires_at = %signin.expires_at,
            "issued a one-time WordPress admin sign-in"
        );
        Ok(signin)
    }

    /// Cancels a pending one-time sign-in link. The file is the whole capability, so
    /// removing it is the whole cancellation.
    pub fn wp_signin_revoke(&self, project_id: &str) -> Result<usize, CoreError> {
        let project = self
            .projects
            .lock()
            .unwrap()
            .get(project_id)
            .ok_or_else(|| CoreError::InvalidProjectPath(project_id.to_string()))?;
        crate::wordpress::revoke(Path::new(&project.path))
    }
}
