use std::collections::BTreeSet;
use std::fs;
use std::process::Command;

use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
pub struct HardwareInfo {
    pub cpu_model: String,
    pub cores: usize,
    pub threads: usize,
    pub gpus: Vec<String>,
    pub memory_total: u64,
    pub kernel: String,
    pub bios: String,
}

pub fn collect() -> HardwareInfo {
    let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let cpu_model = cpuinfo
        .lines()
        .find_map(|line| {
            line.strip_prefix("model name")
                .and_then(|v| v.split_once(':'))
                .map(|(_, v)| v.trim().to_string())
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "未知 CPU".into());

    let threads = cpuinfo
        .lines()
        .filter(|line| line.starts_with("processor"))
        .count()
        .max(1);
    let cores = core_count().unwrap_or(threads);

    HardwareInfo {
        cpu_model,
        cores,
        threads,
        gpus: gpu_names(),
        memory_total: mem_total(),
        kernel: fs::read_to_string("/proc/sys/kernel/osrelease")
            .map(|v| v.trim().to_string())
            .unwrap_or_default(),
        bios: bios(),
    }
}

fn core_count() -> Option<usize> {
    let mut set = BTreeSet::new();
    let entries = fs::read_dir("/sys/devices/system/cpu").ok()?;
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("cpu") || !name[3..].chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let base = entry.path().join("topology");
        let (Ok(package), Ok(core)) = (
            fs::read_to_string(base.join("physical_package_id")),
            fs::read_to_string(base.join("core_id")),
        ) else {
            continue;
        };
        set.insert((package.trim().to_string(), core.trim().to_string()));
    }
    if set.is_empty() {
        None
    } else {
        Some(set.len())
    }
}

fn mem_total() -> u64 {
    let text = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    text.lines()
        .find_map(|line| {
            line.strip_prefix("MemTotal:")
                .and_then(|v| v.split_whitespace().next())
                .and_then(|v| v.parse::<u64>().ok())
        })
        .map(|kb| kb * 1024)
        .unwrap_or(0)
}

fn gpu_names() -> Vec<String> {
    let mut names = Vec::new();

    if let Ok(output) = Command::new("lspci").env("LC_ALL", "C").output()
        && output.status.success()
    {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            for marker in [
                "VGA compatible controller: ",
                "3D controller: ",
                "Display controller: ",
            ] {
                if let Some((_, model)) = line.split_once(marker) {
                    names.push(clean_gpu(model));
                    break;
                }
            }
        }
    }

    if names.is_empty()
        && let Ok(output) = Command::new("nvidia-smi")
            .args(["--query-gpu=name", "--format=csv,noheader"])
            .output()
        && output.status.success()
    {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let line = line.trim();
            if !line.is_empty() {
                names.push(line.to_string());
            }
        }
    }

    names.sort();
    names.dedup();
    if names.is_empty() {
        names.push("未检测到 GPU".into());
    }
    names
}

pub(crate) fn clean_gpu(model: &str) -> String {
    let model = model.split(" (rev ").next().unwrap_or(model).trim();
    let model = model
        .replace("NVIDIA Corporation ", "")
        .replace("Advanced Micro Devices, Inc. [AMD/ATI] ", "")
        .replace("Intel Corporation ", "");
    if let (Some(start), Some(end)) = (model.find('['), model.rfind(']'))
        && end > start
    {
        return model[start + 1..end].trim().to_string();
    }
    model.trim().to_string()
}

fn bios() -> String {
    let version = fs::read_to_string("/sys/class/dmi/id/bios_version")
        .map(|v| v.trim().to_string())
        .unwrap_or_default();
    let date = fs::read_to_string("/sys/class/dmi/id/bios_date")
        .map(|v| v.trim().to_string())
        .unwrap_or_default();
    match (version.is_empty(), date.is_empty()) {
        (false, false) => format!("{version} ({date})"),
        (false, true) => version,
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_gpu_names() {
        assert_eq!(
            clean_gpu("NVIDIA Corporation AD107M [GeForce RTX 4060 Max-Q / Mobile] (rev a1)"),
            "GeForce RTX 4060 Max-Q / Mobile"
        );
        assert_eq!(
            clean_gpu("Advanced Micro Devices, Inc. [AMD/ATI] HawkPoint1 (rev c5)"),
            "HawkPoint1"
        );
    }
}
