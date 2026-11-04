use cf_unwrapper::{meta::Config, xml};
use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
pub fn generate(path: &Path) -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!(
        "cf-extension-fixture-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).map_err(|e| e.to_string())?;
    let result = (|| {
        fs::write(dir.join("Configuration.xml"), configuration_xml()).map_err(|e| e.to_string())?;
        let imported = xml::imp::import(&dir, None)?;
        if !imported.refused.is_empty() {
            return Err(format!("synthetic import refused: {:?}", imported.refused));
        }
        let mut cfg = Config::open(&xml::imp::cf(&dir, &imported)?)?;
        let mut names = xml::edit::Names::new(&cfg);
        let (id, changes) = xml::edit::add(&cfg, &mut names, "Catalog")?;
        xml::edit::apply(&mut cfg, &changes, false)?;
        names = xml::edit::Names::new(&cfg);
        xml::edit::rename(&mut cfg, &mut names, &id, "Products")?;
        for kind in ["Attribute", "TabularSection"] {
            names = xml::edit::Names::new(&cfg);
            let (_, changes) = xml::edit::add_child(&cfg, &mut names, &id, kind)?;
            xml::edit::apply(&mut cfg, &changes, false)?;
        }
        for (owner, roles) in [
            (
                cfg.root_id.clone(),
                vec![
                    "OrdinaryApplicationModule",
                    "ManagedApplicationModule",
                    "SessionModule",
                    "ExternalConnectionModule",
                ],
            ),
            (id.clone(), vec!["ObjectModule", "ManagerModule"]),
        ] {
            for role in roles {
                let change = xml::edit::new_module(&cfg, &owner, role)?;
                xml::edit::apply(&mut cfg, &[change], false)?;
            }
        }
        names = xml::edit::Names::new(&cfg);
        let (_, changes) = xml::edit::add(&cfg, &mut names, "CommonModule")?;
        xml::edit::apply(&mut cfg, &changes, false)?;
        names = xml::edit::Names::new(&cfg);
        let (_, changes) = xml::edit::add_form(&cfg, &mut names, &id, "", false, None, None)?;
        xml::edit::apply(&mut cfg, &changes, false)?;
        for (entry, _) in cfg.module_entries() {
            cfg.set_module_text(
                &entry,
                "// Synthetic module\r\nProcedure Example() Export\r\nEndProcedure",
            )?;
        }
        fs::write(path, cfg.save()).map_err(|e| e.to_string())
    })();
    let _ = fs::remove_dir_all(&dir);
    result
}
// This deliberately synthetic XML uses public XML element names and arbitrary UUIDs.
// It contains no copied seed, template, corporate data or serialized brace records.
fn configuration_xml() -> String {
    let envelope = String::from_utf8(xml::w::W::metadata().finish()).unwrap();
    let mut xml = envelope
        .trim_start_matches('\u{feff}')
        .split("</MetaDataObject>")
        .next()
        .unwrap()
        .to_string();
    xml.push_str(r#"<Configuration uuid="5d812d86-341a-4f5e-a71b-0ee59b35a812"><InternalInfo>"#);
    for (index, class) in [
        "9cd510cd-abfc-11d4-9434-004095e12fc7",
        "9fcd25a0-4822-11d4-9414-008048da11f9",
        "e3687481-0a87-462c-a166-9f34594f9bba",
        "9de14907-ec23-4a07-96f0-85521cb6b53b",
        "51f2d5d8-ea4d-4064-8892-82951750031e",
        "e68182ea-4237-4383-967f-90c1e3370bc7",
        "fb282519-d103-4dd3-bc12-cb271d631dfc",
    ]
    .iter()
    .enumerate()
    {
        xml.push_str(&format!("<xr:ContainedObject><xr:ClassId>{class}</xr:ClassId><xr:ObjectId>88e0d100-acc0-4dda-b001-{index:012}</xr:ObjectId></xr:ContainedObject>"));
    }
    xml.push_str("</InternalInfo><Properties>");
    for tag in "Synonym Comment DefaultRoles NamePrefix BriefInformation DetailedInformation Copyright VendorInformationAddress ConfigurationInformationAddress DefaultInterface DefaultLanguage DefaultStyle Vendor Version UpdateCatalogAddress AdditionalFullTextSearchDictionaries Content CommonSettingsStorage ReportsUserSettingsStorage ReportsVariantsStorage FormDataSettingsStorage StandaloneConfigurationRestrictionRoles DefaultReportForm DefaultReportVariantForm DefaultReportSettingsForm DefaultReportAppearanceTemplate DefaultDynamicListSettingsForm DefaultSearchForm RequiredMobileApplicationPermissions DefaultDataHistoryChangeHistoryForm DefaultDataHistoryVersionDataForm DefaultDataHistoryVersionDifferencesForm DynamicListsUserSettingsStorage MobileApplicationURLs URLExternalDataStorage AllowedIncomingShareRequestTypes DefaultCollaborationSystemUsersChoiceForm DefaultConstantsForm AuxiliaryReportForm AuxiliaryReportVariantForm AuxiliaryReportSettingsForm AuxiliaryDynamicListSettingsForm AuxiliaryDataHistoryChangeHistoryForm AuxiliaryDataHistoryVersionDataForm AuxiliaryDataHistoryVersionDifferencesForm AuxiliaryCollaborationSystemUsersChoiceForm Caption ShortCaption UsePurposes".split_whitespace() { xml.push_str(&format!("<{tag}/>")); }
    for (tag, value) in [
        ("Name", "NavigatorSynthetic"),
        ("CompatibilityMode", "Version8_3_24"),
        ("ConfigurationExtensionCompatibilityMode", "Version8_3_24"),
        ("InterfaceCompatibilityMode", "Taxi"),
        ("ScriptVariant", "English"),
        ("IncludeHelpInContents", "false"),
        ("DataLockControlMode", "Automatic"),
        ("ObjectAutonumerationMode", "AutoFree"),
        ("DefaultRunMode", "ManagedApplication"),
        ("UseManagedFormInOrdinaryApplication", "false"),
        ("UseOrdinaryFormInManagedApplication", "false"),
        ("ModalityUseMode", "DontUse"),
        ("SynchronousPlatformExtensionAndAddInCallUseMode", "DontUse"),
        ("MainClientApplicationWindowMode", "Normal"),
        ("DatabaseTablespacesUseMode", "DontUse"),
        ("ClientApplicationTheme", "Auto"),
        (
            "MainClientApplicationWindowInterfaceVariant",
            "NavigationLeft",
        ),
        ("ClientApplicationWindowsOpenVariant", "OpenDataInTabs"),
        ("Version85InterfaceMigrationMode", "Use"),
    ] {
        xml.push_str(&format!("<{tag}>{value}</{tag}>"));
    }
    xml.push_str("<UsedMobileApplicationFunctionalities>");
    for feature in "Biometrics Location BackgroundLocation BluetoothPrinters WiFiPrinters Contacts Calendars PushNotifications LocalNotifications InAppPurchases PersonalComputerFileExchange Ads NumberDialing CallProcessing CallLog AutoSendSMS ReceiveSMS SMSLog Camera Microphone MusicLibrary PictureAndVideoLibraries AudioPlaybackAndVibration BackgroundAudioPlaybackAndVibration InstallPackages OSBackup ApplicationUsageStatistics BarcodeScanning BackgroundAudioRecording AllFilesAccess Videoconferences NFC DocumentScanning SpeechToText Geofences IncomingShareRequests AllIncomingShareRequestsTypesProcessing TextToSpeech".split_whitespace() { xml.push_str(&format!("<app:functionalities><app:functionality>{feature}</app:functionality><app:use>false</app:use></app:functionalities>")); }
    xml.push_str("</UsedMobileApplicationFunctionalities></Properties><ChildObjects/></Configuration></MetaDataObject>");
    xml
}
