use crate::platform::*;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateCandidate {
    pub request_id: Uuid,
    pub name: String,
    pub image: String,
    pub cpu_milli: i32,
    pub memory_mib: i32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AdoptTemplate {
    pub request_id: Uuid,
    pub name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Activate {
    pub expected_version: i64,
}

pub(super) fn valid_id(id: &str) -> Result<&str> {
    if !id.is_empty()
        && id.len() < 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        Ok(id)
    } else {
        Err(Error::bad("Invalid Cube identifier"))
    }
}

pub(super) fn validate_name(name: &str) -> Result<&str> {
    if (3..=80).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'-' | b'_' | b'.'))
        && name.trim() == name
    {
        Ok(name)
    } else {
        Err(Error::bad("Invalid runtime candidate name"))
    }
}

pub(super) fn validate_image(image: &str) -> Result<()> {
    let configured_prefix = env(
        "FORGE_RUNTIME_IMAGE_PREFIX",
        "127.0.0.1:5000/forge-agent-runtime:",
    );
    let Some((name, digest)) = image.rsplit_once("@sha256:") else {
        return Err(Error::bad("Runtime image must include a SHA-256 digest"));
    };
    let repository = configured_prefix.strip_suffix(':').unwrap_or(&configured_prefix);
    let approved_name = (name.starts_with(&configured_prefix)
        && name.len() > configured_prefix.len())
        || name == repository;
    if !approved_name
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"./:_-".contains(&byte))
        || digest.len() != 64
        || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(Error::bad(
            "Runtime image is outside the approved registry or invalid",
        ));
    }
    Ok(())
}

pub(super) fn validate_spec(input: &CreateCandidate) -> Result<()> {
    validate_name(&input.name)?;
    validate_image(&input.image)?;
    if !(250..=2000).contains(&input.cpu_milli) || !(512..=4096).contains(&input.memory_mib) {
        return Err(Error::bad(
            "Runtime CPU or memory is outside allowed bounds",
        ));
    }
    Ok(())
}

pub(super) fn cube_build_request(candidate: &Value) -> Value {
    json!({
        "name":format!("forge-{}", candidate["id"].as_str().unwrap()),
        "image":candidate["image"],
        "cpu":candidate["cpu_milli"],
        "memory":candidate["memory_mib"],
        "writableLayerSize":"20Gi",
        "exposedPorts":[49999],
        "probePort":49999,
        "probePath":"/health/ready",
        "command":["/usr/local/bin/resident-start.sh"],
        "env":["CUBE_TEMPLATE_WARMUP=1","AGENT_RUNTIME_MODE=resident","AGENT_RUNTIME_BIND=0.0.0.0:49999","HOME=/home/forge","JCODE_HOME=/home/forge/jcode"],
        "networkType":"tap",
        "allowInternetAccess":false
    })
}

pub(super) fn adopted_spec(template: &Value, summary: &Value) -> Result<(String, i32, i32)> {
    if template["status"] != "READY" {
        return Err(Error::bad("Cube template is not ready"));
    }
    let image = summary["imageInfo"]
        .as_str()
        .ok_or_else(|| Error::bad("Cube image is unknown"))?;
    validate_image(image)?;
    let request = &template["createRequest"];
    let containers = request["containers"]
        .as_array()
        .ok_or_else(|| Error::bad("Cube template containers are unknown"))?;
    if containers.len() != 1 || request["network_type"] != "tap" {
        return Err(Error::bad(
            "Cube template does not implement the Forge resident protocol",
        ));
    }
    let container = &containers[0];
    let cpu = container["resources"]["cpu"]
        .as_str()
        .and_then(|v| v.strip_suffix('m'))
        .and_then(|v| v.parse::<i32>().ok());
    let memory = container["resources"]["mem"]
        .as_str()
        .and_then(|v| v.strip_suffix("Mi"))
        .and_then(|v| v.parse::<i32>().ok());
    let port = container["probe"]["probe_handler"]["http_get"]["port"].as_i64();
    let required_env = [
        ("CUBE_TEMPLATE_WARMUP", "1"),
        ("AGENT_RUNTIME_MODE", "resident"),
        ("AGENT_RUNTIME_BIND", "0.0.0.0:49999"),
        ("HOME", "/home/forge"),
        ("JCODE_HOME", "/home/forge/jcode"),
    ];
    let env_matches = container["envs"].as_array().is_some_and(|envs| {
        envs.len() == required_env.len()
            && required_env.iter().all(|(key, value)| {
                envs.iter()
                    .filter(|entry| entry["key"] == *key && entry["value"] == *value)
                    .count()
                    == 1
            })
    });
    if container["command"] != json!(["/usr/local/bin/resident-start.sh"])
        || port != Some(49999)
        || container["probe"]["probe_handler"]["http_get"]["path"] != "/health/ready"
        || !env_matches
    {
        return Err(Error::bad(
            "Cube template does not implement the Forge resident protocol",
        ));
    }
    let (Some(cpu), Some(memory)) = (cpu, memory) else {
        return Err(Error::bad("Cube template resources are unknown"));
    };
    if !(250..=2000).contains(&cpu) || !(512..=4096).contains(&memory) {
        return Err(Error::bad(
            "Cube template resources are outside allowed bounds",
        ));
    }
    Ok((image.to_owned(), cpu, memory))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_and_build_contract_are_closed() {
        assert!(validate_image("127.0.0.1:5000/forge-agent-runtime:v1@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").is_ok());
        assert!(validate_image("127.0.0.1:5000/forge-agent-runtime@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").is_ok());
        for image in [
            "docker.io/other:v1@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "127.0.0.1:5000/forge-agent-runtime:v1",
            "127.0.0.1:5000/forge-agent-runtime:v1@sha256:short",
        ] {
            assert!(validate_image(image).is_err());
        }
        let request = cube_build_request(
            &json!({"id":Uuid::nil().to_string(),"image":"image","cpu_milli":750,"memory_mib":1024}),
        );
        assert_eq!(request["allowInternetAccess"], false);
        assert_eq!(request["probePort"], 49999);
        assert!(request.get("allowOut").is_none());
        assert!(request.get("registryPassword").is_none());
    }

    #[test]
    fn existing_cube_template_uses_the_same_resident_contract() {
        let image = "127.0.0.1:5000/forge-agent-runtime:canary@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let summary = json!({"imageInfo":image});
        let mut detail = json!({"status":"READY","createRequest":{
            "network_type":"tap","containers":[{
                "resources":{"cpu":"750m","mem":"1024Mi"},
                "command":["/usr/local/bin/resident-start.sh"],
                "probe":{"probe_handler":{"http_get":{"port":49999,"path":"/health/ready"}}},
                "envs":[
                    {"key":"CUBE_TEMPLATE_WARMUP","value":"1"},
                    {"key":"AGENT_RUNTIME_MODE","value":"resident"},
                    {"key":"AGENT_RUNTIME_BIND","value":"0.0.0.0:49999"},
                    {"key":"HOME","value":"/home/forge"},
                    {"key":"JCODE_HOME","value":"/home/forge/jcode"}
                ],
                "security_context":{"privileged":true}
            }]
        }});
        assert_eq!(adopted_spec(&detail, &summary).unwrap(), (image.into(), 750, 1024));
        detail["createRequest"]["containers"][0]["command"] = json!(["/bin/sh"]);
        assert!(adopted_spec(&detail, &summary).is_err());
    }
}
