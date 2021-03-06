// Copyright 2020 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use super::helpers::download_from_s3_and_install_bin;
use crate::APP_ID;
use directories::BaseDirs;
use envy::from_env;
use log::info;
use prettytable::Table;
use rpassword;
use safe_api::{
    AuthAllowPrompt, AuthdStatus, AuthedAppsList, PendingAuthReqs, Safe, SafeAuthdClient,
};
use serde::Deserialize;
use std::{fs::File, path::PathBuf};

const AUTH_REQS_NOTIFS_ENDPOINT: &str = "https://localhost:33001";
const ENV_VAR_SAFE_AUTHD_PATH: &str = "SAFE_AUTHD_PATH";

#[cfg(not(target_os = "windows"))]
const SAFE_AUTHD_EXECUTABLE: &str = "safe-authd";

#[cfg(target_os = "windows")]
const SAFE_AUTHD_EXECUTABLE: &str = "safe-authd.exe";

#[derive(Deserialize, Debug)]
struct Environment {
    safe_auth_passphrase: Option<String>,
    safe_auth_password: Option<String>,
}

#[derive(Deserialize, Debug)]
struct LoginDetails {
    pub passphrase: String,
    pub password: String,
}

pub fn authd_install(
    safe_authd: &SafeAuthdClient,
    authd_path: Option<String>,
) -> Result<(), String> {
    let target_path = get_authd_bin_path(authd_path)?;
    let final_path = download_from_s3_and_install_bin(
        target_path,
        "safe-api",
        "safe-authd",
        SAFE_AUTHD_EXECUTABLE,
        None,
    )?;
    if cfg!(windows) {
        // On Windows authd must be installed as a service
        safe_authd
            .install(Some(&final_path))
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

pub fn authd_uninstall(
    safe_authd: &SafeAuthdClient,
    authd_path: Option<String>,
) -> Result<(), String> {
    safe_authd
        .uninstall(authd_path.as_deref())
        .map_err(|err| err.to_string())
}

pub fn authd_update(
    safe_authd: &SafeAuthdClient,
    authd_path: Option<String>,
) -> Result<(), String> {
    safe_authd
        .update(authd_path.as_deref())
        .map_err(|err| err.to_string())
}

pub fn authd_start(safe_authd: &SafeAuthdClient, authd_path: Option<String>) -> Result<(), String> {
    safe_authd
        .start(authd_path.as_deref())
        .map_err(|err| err.to_string())
}

pub fn authd_stop(safe_authd: &SafeAuthdClient, authd_path: Option<String>) -> Result<(), String> {
    safe_authd
        .stop(authd_path.as_deref())
        .map_err(|err| err.to_string())
}

pub fn authd_restart(
    safe_authd: &SafeAuthdClient,
    authd_path: Option<String>,
) -> Result<(), String> {
    safe_authd
        .restart(authd_path.as_deref())
        .map_err(|err| err.to_string())
}

pub async fn authd_create(
    safe: &mut Safe,
    safe_authd: &SafeAuthdClient,
    config_file_str: Option<String>,
    sk: Option<String>,
    test_coins: bool,
) -> Result<(), String> {
    let login_details = get_login_details(config_file_str)?;
    if test_coins {
        // We then generate a SafeKey with test-coins to use it for the account creation
        println!("Creating a SafeKey with test-coins...");
        let (_xorurl, key_pair) = safe.keys_create_preload_test_coins("1000.11").await?;
        let kp = key_pair.ok_or("Faild to obtain the secret key of the newly created SafeKey")?;
        println!("Sending account creation request to authd...");
        safe_authd
            .create_acc(&kp.sk, &login_details.passphrase, &login_details.password)
            .await?;
        println!("Account was created successfully!");
        println!("SafeKey created and preloaded with test-coins. Owner key pair generated:");
        println!("Public Key = {}", kp.pk);
        println!("Secret Key = {}", kp.sk);
    } else {
        let sk = prompt_sensitive(sk, "Enter SafeKey's secret key to pay with:")?;
        println!("Sending account creation request to authd...");
        safe_authd
            .create_acc(&sk, &login_details.passphrase, &login_details.password)
            .await?;
        println!("Account was created successfully!");
    };
    Ok(())
}

pub async fn authd_login(
    safe_authd: &mut SafeAuthdClient,
    config_file_str: Option<String>,
) -> Result<(), String> {
    let login_details = get_login_details(config_file_str)?;
    println!("Sending login action request to authd...");
    safe_authd
        .log_in(&login_details.passphrase, &login_details.password)
        .await?;
    println!("Logged in successfully");
    Ok(())
}

pub async fn authd_logout(safe_authd: &mut SafeAuthdClient) -> Result<(), String> {
    println!("Sending logout action request to authd...");
    safe_authd.log_out().await?;
    println!("Logged out successfully");
    Ok(())
}

pub async fn authd_status(safe_authd: &mut SafeAuthdClient) -> Result<(), String> {
    println!("Sending request to authd to obtain a status report...");
    let status_report = safe_authd.status().await?;
    pretty_print_status_report(status_report);

    Ok(())
}

pub async fn authd_apps(safe_authd: &SafeAuthdClient) -> Result<(), String> {
    println!("Requesting list of authorised apps from authd...");
    let authed_apps = safe_authd.authed_apps().await?;
    pretty_print_authed_apps(authed_apps);

    Ok(())
}

pub async fn authd_revoke(safe_authd: &SafeAuthdClient, app_id: String) -> Result<(), String> {
    println!("Sending application revocation request to authd...");
    safe_authd.revoke_app(&app_id).await?;
    println!("Application revoked successfully");
    Ok(())
}

pub async fn authd_auth_reqs(safe_authd: &SafeAuthdClient) -> Result<(), String> {
    println!("Requesting list of pending authorisation requests from authd...");
    let auth_reqs = safe_authd.auth_reqs().await?;
    pretty_print_auth_reqs(auth_reqs, Some("Pending Authorisation requests"));
    Ok(())
}

pub async fn authd_allow(safe_authd: &SafeAuthdClient, req_id: u32) -> Result<(), String> {
    println!("Sending request to authd to allow an authorisation request...");
    safe_authd.allow(req_id).await?;
    println!("Authorisation request was allowed successfully");
    Ok(())
}

pub async fn authd_deny(safe_authd: &SafeAuthdClient, req_id: u32) -> Result<(), String> {
    println!("Sending request to authd to deny an authorisation request...");
    safe_authd.deny(req_id).await?;
    println!("Authorisation request was denied successfully");
    Ok(())
}

pub async fn authd_subscribe(
    safe_authd: &mut SafeAuthdClient,
    notifs_endpoint: Option<String>,
    auth_allow_prompt: &'static AuthAllowPrompt,
) -> Result<(), String> {
    println!("Sending request to subscribe...");
    let endpoint = notifs_endpoint.unwrap_or_else(|| AUTH_REQS_NOTIFS_ENDPOINT.to_string());
    safe_authd
        .subscribe(&endpoint, APP_ID, auth_allow_prompt)
        .await?;
    println!("Subscribed successfully");
    Ok(())
}

#[allow(dead_code)]
pub async fn authd_subscribe_url(
    safe_authd: &SafeAuthdClient,
    notifs_endpoint: String,
) -> Result<(), String> {
    println!("Sending request to subscribe URL...");
    safe_authd.subscribe_url(&notifs_endpoint).await?;
    println!("URL subscribed successfully");
    Ok(())
}

pub async fn authd_unsubscribe(
    safe_authd: &mut SafeAuthdClient,
    notifs_endpoint: Option<String>,
) -> Result<(), String> {
    println!("Sending request to unsubscribe...");
    let endpoint = notifs_endpoint.unwrap_or_else(|| AUTH_REQS_NOTIFS_ENDPOINT.to_string());
    safe_authd.unsubscribe(&endpoint).await?;
    println!("Unsubscribed successfully");
    Ok(())
}

pub fn pretty_print_authed_apps(authed_apps: AuthedAppsList) {
    let mut table = Table::new();
    table.add_row(row![bFg->"Authorised Applications"]);
    table.add_row(row![bFg->"Id", bFg->"Name", bFg->"Vendor", bFg->"Permissions"]);
    let all_app_iterator = authed_apps.iter();
    for authed_app in all_app_iterator {
        let mut containers_perms = String::default();
        for (cont, perms) in authed_app.containers.iter() {
            containers_perms += &format!("{}: {:?}\n", cont, perms);
        }
        if containers_perms.is_empty() {
            containers_perms = "None".to_string();
        }

        let app_permissions = format!(
            "Transfer coins: {}\nMutations: {}\nRead coin balance: {}",
            boolean_to_string(authed_app.app_permissions.transfer_coins),
            boolean_to_string(authed_app.app_permissions.perform_mutations),
            boolean_to_string(authed_app.app_permissions.get_balance)
        );
        let permissions_report = format!(
            "Own container: {}\n{}\nContainers: {}",
            boolean_to_string(authed_app.own_container),
            app_permissions,
            containers_perms
        );

        table.add_row(row![
            authed_app.id,
            authed_app.name,
            authed_app.vendor,
            permissions_report
        ]);
    }
    table.printstd();
}

pub fn pretty_print_auth_reqs(auth_reqs: PendingAuthReqs, title_msg: Option<&str>) {
    if auth_reqs.is_empty() {
        println!("There are no pending authorisation requests");
    } else {
        let mut table = Table::new();
        if let Some(title) = title_msg {
            table.add_row(row![bFg->title]);
        }
        table.add_row(
            row![bFg->"Request Id", bFg->"App Id", bFg->"Name", bFg->"Vendor", bFg->"Permissions requested"],
        );
        for auth_req in auth_reqs.iter() {
            let mut containers_perms = String::default();
            for (cont, perms) in auth_req.containers.iter() {
                containers_perms += &format!("{}: {:?}\n", cont, perms);
            }
            if containers_perms.is_empty() {
                containers_perms = "None".to_string();
            }

            let app_permissions = format!(
                "Transfer coins: {}\nMutations: {}\nRead coin balance: {}",
                boolean_to_string(auth_req.app_permissions.transfer_coins),
                boolean_to_string(auth_req.app_permissions.perform_mutations),
                boolean_to_string(auth_req.app_permissions.get_balance)
            );
            let permissions_report = format!(
                "Own container: {}\n{}\nContainers: {}",
                boolean_to_string(auth_req.own_container),
                app_permissions,
                containers_perms
            );

            table.add_row(row![
                auth_req.req_id,
                auth_req.app_id,
                auth_req.app_name,
                auth_req.app_vendor,
                permissions_report,
            ]);
        }
        table.printstd();
    }
}

pub fn pretty_print_status_report(status_report: AuthdStatus) {
    let mut table = Table::new();
    table.add_row(row![bFg->"SAFE Authenticator status"]);
    table.add_row(row![
        "Authenticator daemon version",
        status_report.authd_version
    ]);
    table.add_row(row![
        "Logged in to a SAFE account?",
        boolean_to_string(status_report.logged_in),
    ]);
    table.add_row(row![
        "Number of pending authorisation requests",
        status_report.num_auth_reqs,
    ]);
    table.add_row(row![
        "Number of notifications subscribers",
        status_report.num_notif_subs,
    ]);
    table.printstd();
}

// Private helpers

#[inline]
fn boolean_to_string(boolean: bool) -> &'static str {
    if boolean {
        "Yes"
    } else {
        "No"
    }
}

fn prompt_sensitive(arg: Option<String>, msg: &str) -> Result<String, String> {
    if let Some(str) = arg {
        Ok(str)
    } else {
        rpassword::read_password_from_tty(Some(msg))
            .map_err(|err| format!("Failed reading string from input: {}", err))
    }
}

fn get_login_details(config_file: Option<String>) -> Result<LoginDetails, String> {
    let environment_details = from_env::<Environment>().map_err(|err| {
        format!(
            "Failed when attempting to read login details from env vars: {}",
            err
        )
    })?;

    let mut the_passphrase = environment_details
        .safe_auth_passphrase
        .unwrap_or_else(|| String::from(""));
    if !the_passphrase.is_empty() {
        info!("Using passphrase from provided ENV var: SAFE_AUTH_PASSPHRASE")
    }

    let mut the_password = environment_details
        .safe_auth_password
        .unwrap_or_else(|| String::from(""));
    if !the_password.is_empty() {
        info!("Using password from provided ENV var: SAFE_AUTH_PASSWORD")
    }

    if the_passphrase.is_empty() ^ the_password.is_empty() {
        return Err("Both the passphrase (SAFE_AUTH_PASSPHRASE) and password (SAFE_AUTH_PASSWORD) environment variables must be set for SAFE account creation/login.".to_string());
    }

    if the_passphrase.is_empty() || the_password.is_empty() {
        if let Some(config_file_str) = config_file {
            let file = match File::open(&config_file_str) {
                Ok(file) => file,
                Err(error) => {
                    return Err(format!("Error reading config file: {}", error));
                }
            };

            let json: LoginDetails = serde_json::from_reader(file).map_err(|err| {
                format!(
                    "Format of the config file is not valid and couldn't be parsed: {}",
                    err
                )
            })?;

            eprintln!("Warning! Storing your passphrase/password in plaintext in a config file is not secure." );

            if json.passphrase.is_empty() {
                return Err("The config files's passphrase field cannot be empty".to_string());
            } else {
                the_passphrase = json.passphrase;
            }

            if json.password.is_empty() {
                return Err("The config files's password field cannot be empty".to_string());
            } else {
                the_password = json.password;
            }
        } else {
            // Prompt the user for the SAFE account credentials
            the_passphrase = prompt_sensitive(None, "Passphrase: ")
                .map_err(|err| format!("Failed reading 'passphrase' string from input: {}", err))?;
            the_password = prompt_sensitive(None, "Password: ")
                .map_err(|err| format!("Failed reading 'passphrase' string from input: {}", err))?;
        }
    }

    if the_passphrase.is_empty() || the_password.is_empty() {
        return Err(String::from(
            "Neither the passphrase nor password can be empty.",
        ));
    }

    let details = LoginDetails {
        passphrase: the_passphrase,
        password: the_password,
    };

    Ok(details)
}

#[inline]
fn get_authd_bin_path(authd_path: Option<String>) -> Result<PathBuf, String> {
    match authd_path {
        Some(p) => Ok(PathBuf::from(p)),
        None => {
            // if SAFE_AUTHD_PATH is set it then overrides default
            if let Ok(authd_path) = std::env::var(ENV_VAR_SAFE_AUTHD_PATH) {
                Ok(PathBuf::from(authd_path))
            } else {
                let base_dirs = BaseDirs::new()
                    .ok_or_else(|| "Failed to obtain user's home path".to_string())?;

                let mut path = PathBuf::from(base_dirs.home_dir());
                path.push(".safe");
                path.push("authd");
                Ok(path)
            }
        }
    }
}
