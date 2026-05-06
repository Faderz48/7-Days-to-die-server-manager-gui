use eframe::egui;
use rfd::FileDialog;
use std::{
    collections::BTreeSet,
    env,
    ffi::OsStr,
    fs::{self, File},
    io::{self, BufReader},
    path::{Component, Path, PathBuf},
};
use zip::ZipArchive;

const APP_TITLE: &str = "7 Days to Die Mod Manager";
const GAME_FOLDER_NAME: &str = "7 Days To Die";
const MODS_FOLDER: &str = "Mods";
const DISABLED_FOLDER: &str = "DisabledMods";

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([820.0, 560.0]),
        ..Default::default()
    };

    eframe::run_native(
        APP_TITLE,
        options,
        Box::new(|_cc| Box::new(ModManagerApp::new())),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ModState {
    Enabled,
    Disabled,
}

struct ManagedMod {
    name: String,
    path: PathBuf,
    state: ModState,
}

struct ModManagerApp {
    game_dir: Option<PathBuf>,
    mods: Vec<ManagedMod>,
    selected_mod: Option<usize>,
    status: String,
}

impl ModManagerApp {
    fn new() -> Self {
        let mut app = Self {
            game_dir: load_saved_game_dir().or_else(auto_detect_game_dir),
            mods: Vec::new(),
            selected_mod: None,
            status: String::new(),
        };

        if let Some(game_dir) = app.game_dir.clone() {
            app.status = format!("Using game folder: {}", game_dir.display());
            app.refresh_mods();
        } else {
            app.status = "Choose your 7 Days to Die folder to get started.".to_owned();
        }

        app
    }

    fn choose_game_folder(&mut self) {
        if let Some(path) = FileDialog::new()
            .set_title("Choose the 7 Days to Die game folder")
            .pick_folder()
        {
            if looks_like_game_folder(&path) {
                self.game_dir = Some(path.clone());
                self.selected_mod = None;
                save_game_dir(&path);
                self.ensure_mod_folders();
                self.refresh_mods();
                self.status = format!("Game folder set to {}", path.display());
            } else {
                self.status = format!(
                    "That does not look like a 7 Days to Die folder: {}",
                    path.display()
                );
            }
        }
    }

    fn install_zip(&mut self) {
        let Some(game_dir) = self.game_dir.clone() else {
            self.status = "Choose the game folder before installing mods.".to_owned();
            return;
        };

        let Some(zip_path) = FileDialog::new()
            .set_title("Choose a mod ZIP")
            .add_filter("ZIP files", &["zip"])
            .pick_file()
        else {
            return;
        };

        match install_mod_zip(&game_dir, &zip_path) {
            Ok(name) => {
                self.refresh_mods();
                self.status = format!("Installed mod: {name}");
            }
            Err(err) => {
                self.status = format!("Could not install {}: {err}", zip_path.display());
            }
        }
    }

    fn enable_selected(&mut self) {
        let Some(index) = self.selected_mod else {
            self.status = "Select a disabled mod first.".to_owned();
            return;
        };

        if self.mods[index].state == ModState::Enabled {
            self.status = "That mod is already enabled.".to_owned();
            return;
        }

        let Some(game_dir) = self.game_dir.clone() else {
            return;
        };

        let name = self.mods[index].name.clone();
        let from = game_dir.join(DISABLED_FOLDER).join(&name);
        let to = game_dir.join(MODS_FOLDER).join(&name);

        match move_mod_folder(&from, &to) {
            Ok(()) => {
                self.refresh_mods();
                self.status = format!("Enabled mod: {name}");
            }
            Err(err) => self.status = format!("Could not enable {name}: {err}"),
        }
    }

    fn disable_selected(&mut self) {
        let Some(index) = self.selected_mod else {
            self.status = "Select an enabled mod first.".to_owned();
            return;
        };

        if self.mods[index].state == ModState::Disabled {
            self.status = "That mod is already disabled.".to_owned();
            return;
        }

        let Some(game_dir) = self.game_dir.clone() else {
            return;
        };

        let name = self.mods[index].name.clone();
        let from = game_dir.join(MODS_FOLDER).join(&name);
        let to = game_dir.join(DISABLED_FOLDER).join(&name);

        match move_mod_folder(&from, &to) {
            Ok(()) => {
                self.refresh_mods();
                self.status = format!("Disabled mod: {name}");
            }
            Err(err) => self.status = format!("Could not disable {name}: {err}"),
        }
    }

    fn remove_selected(&mut self) {
        let Some(index) = self.selected_mod else {
            self.status = "Select a mod to remove.".to_owned();
            return;
        };

        let name = self.mods[index].name.clone();
        let path = self.mods[index].path.clone();

        if is_protected_mod_name(&name) {
            self.status = format!("{name} is protected and cannot be deleted.");
            return;
        }

        match fs::remove_dir_all(&path) {
            Ok(()) => {
                self.selected_mod = None;
                self.refresh_mods();
                self.status = format!("Removed mod: {name}");
            }
            Err(err) => self.status = format!("Could not remove {name}: {err}"),
        }
    }

    fn ensure_mod_folders(&self) {
        if let Some(game_dir) = &self.game_dir {
            let _ = fs::create_dir_all(game_dir.join(MODS_FOLDER));
            let _ = fs::create_dir_all(game_dir.join(DISABLED_FOLDER));
        }
    }

    fn refresh_mods(&mut self) {
        self.mods.clear();
        self.selected_mod = None;

        let Some(game_dir) = &self.game_dir else {
            return;
        };

        self.ensure_mod_folders();
        self.mods
            .extend(read_mods_in(game_dir.join(MODS_FOLDER), ModState::Enabled));
        self.mods.extend(read_mods_in(
            game_dir.join(DISABLED_FOLDER),
            ModState::Disabled,
        ));
        self.mods
            .sort_by_key(|m| (m.state != ModState::Enabled, m.name.to_lowercase()));
    }
}

impl eframe::App for ModManagerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(APP_TITLE);
            ui.add_space(8.0);

            ui.horizontal_wrapped(|ui| {
                ui.label("Game folder:");
                let folder_text = self
                    .game_dir
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "Not selected".to_owned());
                ui.monospace(folder_text);
            });

            ui.horizontal(|ui| {
                if ui.button("Choose Game Folder").clicked() {
                    self.choose_game_folder();
                }

                if ui
                    .add_enabled(self.game_dir.is_some(), egui::Button::new("Install ZIP"))
                    .clicked()
                {
                    self.install_zip();
                }

                if ui
                    .add_enabled(self.game_dir.is_some(), egui::Button::new("Refresh"))
                    .clicked()
                {
                    self.refresh_mods();
                    self.status = "Mod list refreshed.".to_owned();
                }
            });

            ui.separator();

            let has_selection = self.selected_mod.is_some();
            let can_delete_selection = self
                .selected_mod
                .and_then(|index| self.mods.get(index))
                .is_some_and(|managed_mod| !is_protected_mod_name(&managed_mod.name));

            ui.heading("Actions");
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(has_selection, egui::Button::new("Enable"))
                    .clicked()
                {
                    self.enable_selected();
                }
                if ui
                    .add_enabled(has_selection, egui::Button::new("Disable"))
                    .clicked()
                {
                    self.disable_selected();
                }
                if ui
                    .add_enabled(can_delete_selection, egui::Button::new("Remove Mod"))
                    .clicked()
                {
                    self.remove_selected();
                }
            });

            ui.label("Remove Mod permanently deletes the selected mod folder.");

            ui.separator();

            ui.heading("Mods");
            ui.add_space(4.0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height(340.0)
                .show(ui, |ui| {
                    if self.mods.is_empty() {
                        ui.label("No mods found.");
                    }

                    for (index, managed_mod) in self.mods.iter().enumerate() {
                        let state = match managed_mod.state {
                            ModState::Enabled => "Enabled",
                            ModState::Disabled => "Disabled",
                        };
                        let protected = if is_protected_mod_name(&managed_mod.name) {
                            " - Protected"
                        } else {
                            ""
                        };
                        let label = format!("{} ({state}{protected})", managed_mod.name);
                        if ui
                            .selectable_label(self.selected_mod == Some(index), label)
                            .clicked()
                        {
                            self.selected_mod = Some(index);
                        }
                    }
                });

            ui.separator();
            ui.label(&self.status);
        });
    }
}

fn read_mods_in(folder: PathBuf, state: ModState) -> Vec<ManagedMod> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };

    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|entry| ManagedMod {
            name: entry.file_name().to_string_lossy().to_string(),
            path: entry.path(),
            state,
        })
        .collect()
}

fn is_protected_mod_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == "0_tfp_harmony" || lower == ")_tfp_harmony" || lower.ends_with("_tfp_harmony")
}

fn install_mod_zip(game_dir: &Path, zip_path: &Path) -> io::Result<String> {
    fs::create_dir_all(game_dir.join(MODS_FOLDER))?;
    fs::create_dir_all(game_dir.join(DISABLED_FOLDER))?;

    let file = File::open(zip_path)?;
    let mut archive = ZipArchive::new(BufReader::new(file))?;
    let layout = detect_zip_layout(&mut archive, zip_path)?;
    let mod_name = layout.mod_name.clone();

    match layout.mode {
        ZipLayoutMode::SingleRoot => {
            ensure_mod_name_available(game_dir, &mod_name)?;
            for index in 0..archive.len() {
                extract_entry(&mut archive, index, &game_dir.join(MODS_FOLDER), None)?;
            }
        }
        ZipLayoutMode::Rootless => {
            ensure_mod_name_available(game_dir, &mod_name)?;
            let destination = game_dir.join(MODS_FOLDER).join(&mod_name);
            fs::create_dir_all(&destination)?;
            for index in 0..archive.len() {
                extract_entry(&mut archive, index, &destination, None)?;
            }
        }
    }

    Ok(mod_name)
}

fn ensure_mod_name_available(game_dir: &Path, mod_name: &str) -> io::Result<()> {
    let enabled = game_dir.join(MODS_FOLDER).join(mod_name);
    let disabled = game_dir.join(DISABLED_FOLDER).join(mod_name);

    if enabled.exists() || disabled.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("a mod named {mod_name} already exists"),
        ));
    }

    Ok(())
}

struct ZipLayout {
    mode: ZipLayoutMode,
    mod_name: String,
}

enum ZipLayoutMode {
    SingleRoot,
    Rootless,
}

fn detect_zip_layout<R: io::Read + io::Seek>(
    archive: &mut ZipArchive<R>,
    zip_path: &Path,
) -> io::Result<ZipLayout> {
    let mut roots = BTreeSet::new();
    let mut has_root_modinfo = false;
    let mut root_with_modinfo = None;

    for index in 0..archive.len() {
        let file = archive.by_index(index)?;
        let Some(path) = file.enclosed_name() else {
            continue;
        };

        let mut components = path.components().filter_map(component_text);
        let Some(first) = components.next() else {
            continue;
        };
        let second = components.next();

        roots.insert(first.clone());

        if first.eq_ignore_ascii_case("ModInfo.xml") {
            has_root_modinfo = true;
        } else if second
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case("ModInfo.xml"))
        {
            root_with_modinfo = Some(first);
        }
    }

    if has_root_modinfo {
        return Ok(ZipLayout {
            mode: ZipLayoutMode::Rootless,
            mod_name: zip_stem(zip_path),
        });
    }

    if roots.len() == 1 {
        let mod_name = roots
            .into_iter()
            .next()
            .unwrap_or_else(|| zip_stem(zip_path));
        return Ok(ZipLayout {
            mode: ZipLayoutMode::SingleRoot,
            mod_name,
        });
    }

    if let Some(mod_name) = root_with_modinfo {
        return Ok(ZipLayout {
            mode: ZipLayoutMode::SingleRoot,
            mod_name,
        });
    }

    Ok(ZipLayout {
        mode: ZipLayoutMode::Rootless,
        mod_name: zip_stem(zip_path),
    })
}

fn component_text(component: Component<'_>) -> Option<String> {
    match component {
        Component::Normal(text) => Some(text.to_string_lossy().to_string()),
        _ => None,
    }
}

fn extract_entry<R: io::Read + io::Seek>(
    archive: &mut ZipArchive<R>,
    index: usize,
    destination_root: &Path,
    strip_first_component: Option<&str>,
) -> io::Result<()> {
    let mut file = archive.by_index(index)?;
    let Some(enclosed_name) = file.enclosed_name().map(PathBuf::from) else {
        return Ok(());
    };

    let relative_path = if let Some(strip) = strip_first_component {
        strip_component(&enclosed_name, strip).unwrap_or(enclosed_name)
    } else {
        enclosed_name
    };

    if relative_path.as_os_str().is_empty() {
        return Ok(());
    }

    let output_path = destination_root.join(relative_path);

    if file.name().ends_with('/') {
        fs::create_dir_all(output_path)?;
    } else {
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output_file = File::create(output_path)?;
        io::copy(&mut file, &mut output_file)?;
    }

    Ok(())
}

fn strip_component(path: &Path, strip: &str) -> Option<PathBuf> {
    let mut components = path.components();
    let first = components.next()?;

    if component_text(first)?.eq_ignore_ascii_case(strip) {
        Some(components.collect())
    } else {
        None
    }
}

fn move_mod_folder(from: &Path, to: &Path) -> io::Result<()> {
    if to.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("destination already exists: {}", to.display()),
        ));
    }

    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::rename(from, to)
}

fn looks_like_game_folder(path: &Path) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.eq_ignore_ascii_case(GAME_FOLDER_NAME))
        || path.join("7DaysToDie.exe").exists()
}

fn auto_detect_game_dir() -> Option<PathBuf> {
    steam_game_candidates()
        .into_iter()
        .find(|path| looks_like_game_folder(path))
}

fn steam_game_candidates() -> Vec<PathBuf> {
    let mut steam_roots = Vec::new();

    if let Some(program_files_x86) = env::var_os("ProgramFiles(x86)") {
        steam_roots.push(PathBuf::from(program_files_x86).join("Steam"));
    }
    if let Some(program_files) = env::var_os("ProgramFiles") {
        steam_roots.push(PathBuf::from(program_files).join("Steam"));
    }

    let mut candidates = Vec::new();
    for steam_root in steam_roots {
        candidates.push(
            steam_root
                .join("steamapps")
                .join("common")
                .join(GAME_FOLDER_NAME),
        );

        let library_file = steam_root.join("steamapps").join("libraryfolders.vdf");
        for library in read_steam_libraries(&library_file) {
            candidates.push(
                library
                    .join("steamapps")
                    .join("common")
                    .join(GAME_FOLDER_NAME),
            );
        }
    }

    candidates
}

fn read_steam_libraries(path: &Path) -> Vec<PathBuf> {
    let Ok(contents) = fs::read_to_string(path) else {
        return Vec::new();
    };

    contents
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if !trimmed.starts_with("\"path\"") {
                return None;
            }

            let values: Vec<_> = trimmed.split('"').collect();
            values
                .get(3)
                .map(|value| PathBuf::from(value.replace("\\\\", "\\")))
        })
        .collect()
}

fn config_file() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("SevenDaysModManager").join("game_folder.txt"))
}

fn load_saved_game_dir() -> Option<PathBuf> {
    let path = config_file()?;
    let game_dir = PathBuf::from(fs::read_to_string(path).ok()?.trim());

    looks_like_game_folder(&game_dir).then_some(game_dir)
}

fn save_game_dir(game_dir: &Path) {
    if let Some(path) = config_file() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(path, game_dir.display().to_string());
    }
}

fn zip_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(OsStr::to_str)
        .filter(|name| !name.is_empty())
        .unwrap_or("InstalledMod")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::is_protected_mod_name;

    #[test]
    fn protects_tfp_harmony_variants() {
        assert!(is_protected_mod_name("0_TFP_Harmony"));
        assert!(is_protected_mod_name(")_TFP_Harmony"));
        assert!(is_protected_mod_name("_TFP_Harmony"));
        assert!(is_protected_mod_name("0_tfp_harmony"));
    }

    #[test]
    fn does_not_protect_regular_mods() {
        assert!(!is_protected_mod_name("BetterFarming"));
        assert!(!is_protected_mod_name("HarmonyPatchForAnotherMod"));
    }
}
