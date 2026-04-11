use std::ffi::{CString, c_char, c_int, c_long, c_uint, c_void};
use std::path::Path;

use libloading::os::unix::Library;

use crate::error::{AppError, AppResult};

use super::layout::{SharedPtr, StdString, StdVector};

pub type DialogHandler = unsafe extern "C" fn(c_long, *mut SharedPtr, *mut SharedPtr);
pub type CredentialHandler = unsafe extern "C" fn(*mut SharedPtr, *mut SharedPtr);

macro_rules! load_symbol {
    ($name:literal, $ty:ty) => {{ load_global_symbol::<$ty>($name)? }};
}

macro_rules! load_data_symbol {
    ($name:literal) => {{ load_global_symbol::<*mut c_void>($name)? }};
}

type FnResolvSetNameservers =
    unsafe extern "C" fn(c_uint, *const *const c_char, c_int, *const c_char);
type FnFootHillConfig = unsafe extern "C" fn(*const StdString);
type FnDeviceGuidInstance = unsafe extern "C" fn(*mut SharedPtr);
type FnDeviceGuidConfigure = unsafe extern "C" fn(
    *mut [u8; 88],
    *mut c_void,
    *const StdString,
    *const StdString,
    *const u32,
    *const u8,
);
type FnRequestContextMakeShared = unsafe extern "C" fn(*mut SharedPtr, *const StdString);
type FnRequestContextConfigCtor = unsafe extern "C" fn(*mut c_void);
type FnRequestContextConfigSetString = unsafe extern "C" fn(*mut c_void, *const StdString);
type FnRequestContextManagerConfigure = unsafe extern "C" fn(*const SharedPtr);
type FnRequestContextInit = unsafe extern "C" fn(*mut [u8; 88], *mut c_void, *const SharedPtr);
type FnRequestContextSetPresentation = unsafe extern "C" fn(*mut c_void, *const SharedPtr);
type FnRequestContextSetFairPlayDir = unsafe extern "C" fn(*mut c_void, *const StdString);
type FnAndroidPresentationMakeShared = unsafe extern "C" fn(*mut SharedPtr);
type FnSetDialogHandler = unsafe extern "C" fn(*mut c_void, DialogHandler);
type FnSetCredentialHandler = unsafe extern "C" fn(*mut c_void, CredentialHandler);
type FnHandleDialogResponse = unsafe extern "C" fn(*mut c_void, *const c_long, *const SharedPtr);
type FnHandleCredentialsResponse = unsafe extern "C" fn(*mut c_void, *const SharedPtr);
type FnProtocolDialogResponseCtor = unsafe extern "C" fn(*mut c_void);
type FnProtocolDialogResponseSetSelectedButton =
    unsafe extern "C" fn(*mut c_void, *const SharedPtr);
type FnProtocolDialogTitle = unsafe extern "C" fn(*mut c_void) -> *const StdString;
type FnProtocolDialogMessage = unsafe extern "C" fn(*mut c_void) -> *const StdString;
type FnProtocolDialogButtons = unsafe extern "C" fn(*mut c_void) -> *mut StdVectorSharedPtr;
type FnProtocolButtonTitle = unsafe extern "C" fn(*mut c_void) -> *const StdString;
type FnCredentialsRequestTitle = unsafe extern "C" fn(*mut c_void) -> *const StdString;
type FnCredentialsRequestMessage = unsafe extern "C" fn(*mut c_void) -> *const StdString;
type FnCredentialsRequestNeeds2fa = unsafe extern "C" fn(*mut c_void) -> u8;
type FnCredentialsResponseCtor = unsafe extern "C" fn(*mut c_void);
type FnCredentialsResponseSetString = unsafe extern "C" fn(*mut c_void, *const StdString);
type FnCredentialsResponseSetType = unsafe extern "C" fn(*mut c_void, c_int);
type FnAuthenticateFlowMakeShared = unsafe extern "C" fn(*mut SharedPtr, *const SharedPtr);
type FnAuthenticateFlowRun = unsafe extern "C" fn(*mut c_void);
type FnAuthenticateFlowResponse = unsafe extern "C" fn(*mut c_void) -> *mut SharedPtr;
type FnAuthenticateResponseType = unsafe extern "C" fn(*mut c_void) -> c_int;
type FnLeaseManagerCtor = unsafe extern "C" fn(*mut c_void, *const c_void, *const c_void);
type FnLeaseManagerRefresh = unsafe extern "C" fn(*mut c_void, *const u8);
type FnLeaseManagerRequestLease = unsafe extern "C" fn(*mut c_void, *const u8);
type FnLeaseManagerRelease = unsafe extern "C" fn(*mut c_void);
type FnSessionCtrlInstance = unsafe extern "C" fn() -> *mut c_void;
type FnSessionCtrlGetPersistentKey = unsafe extern "C" fn(
    *mut SharedPtr,
    *mut c_void,
    *const StdString,
    *const StdString,
    *const StdString,
    *const StdString,
    *const StdString,
    *const StdString,
    *const StdString,
    *const StdString,
);
type FnSessionCtrlDecryptContext = unsafe extern "C" fn(*mut SharedPtr, *mut c_void, *mut c_void);
type FnPContextKdContext = unsafe extern "C" fn(*mut c_void) -> *mut *mut c_void;
type FnSharedPtrPContextDrop = unsafe extern "C" fn(*mut SharedPtr);
type FnDecryptSample =
    unsafe extern "C" fn(*mut c_void, u32, *mut c_void, *mut c_void, usize) -> c_long;
type FnRequestContextStorefrontIdentifier =
    unsafe extern "C" fn(*mut StdString, *mut c_void, *const SharedPtr) -> *mut StdString;
type FnRequestContextFairPlay = unsafe extern "C" fn(*mut SharedPtr, *mut c_void) -> *mut c_void;
type FnFairPlayGetSubscriptionStatus = unsafe extern "C" fn(*mut c_void) -> StdVector;
type FnPlaybackLeaseManagerRequestAsset =
    unsafe extern "C" fn(*mut SharedPtr, *mut c_void, *const u64, *const StdVector, *const u8);
type FnPlaybackAssetResponseHasValidAsset = unsafe extern "C" fn(*mut c_void) -> c_int;
type FnPlaybackAssetResponsePlaybackAsset = unsafe extern "C" fn(*mut c_void) -> *mut SharedPtr;
type FnPlaybackAssetUrlString = unsafe extern "C" fn(*mut StdString, *mut c_void) -> *mut StdString;
type FnHttpMessageCtor =
    unsafe extern "C" fn(*mut c_void, *const StdString, *const StdString) -> *mut c_void;
type FnHttpMessageSetHeader = unsafe extern "C" fn(*mut c_void, *const StdString, *const StdString);
type FnHttpMessageSetBodyData = unsafe extern "C" fn(*mut c_void, *mut c_char, usize);
type FnDeviceGuidGuid = unsafe extern "C" fn(*mut [*mut c_void; 2], *mut c_void) -> *mut c_void;
type FnDataBytes = unsafe extern "C" fn(*mut c_void) -> *mut c_char;
type FnDataLength = unsafe extern "C" fn(*mut c_void) -> usize;
type FnUrlRequestCtor =
    unsafe extern "C" fn(*mut c_void, *const SharedPtr, *const SharedPtr) -> *mut c_void;
type FnUrlRequestSetParameter =
    unsafe extern "C" fn(*mut c_void, *const StdString, *const StdString) -> *mut c_void;
type FnUrlRequestRun = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type FnUrlRequestError = unsafe extern "C" fn(*mut c_void) -> *mut SharedPtr;
type FnUrlRequestResponse = unsafe extern "C" fn(*mut c_void) -> *mut SharedPtr;
type FnUrlResponseUnderlyingResponse = unsafe extern "C" fn(*mut c_void) -> *mut SharedPtr;
type FnPurchaseRequestCtor = unsafe extern "C" fn(*mut c_void, *const SharedPtr) -> *mut c_void;
type FnPurchaseRequestSetProcessDialogActions = unsafe extern "C" fn(*mut c_void, c_int);
type FnPurchaseRequestSetString =
    unsafe extern "C" fn(*mut c_void, *const StdString) -> *mut c_void;
type FnPurchaseRequestRun = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type FnPurchaseRequestResponse = unsafe extern "C" fn(*mut c_void) -> *mut SharedPtr;
type FnPurchaseResponseError = unsafe extern "C" fn(*mut c_void) -> *mut SharedPtr;
type FnPurchaseResponseItems = unsafe extern "C" fn(*mut c_void) -> StdVector;
type FnPurchaseItemAssets = unsafe extern "C" fn(*mut c_void) -> StdVector;
type FnPurchaseAssetUrl = unsafe extern "C" fn(*mut StdString, *mut c_void) -> *mut StdString;

#[repr(C)]
pub struct StdVectorSharedPtr {
    pub begin: *mut SharedPtr,
    pub end: *mut SharedPtr,
    pub end_capacity: *mut SharedPtr,
}

pub struct NativeSymbols {
    pub resolv_set_nameservers: FnResolvSetNameservers,
    pub foothill_config: FnFootHillConfig,
    pub device_guid_instance: FnDeviceGuidInstance,
    pub device_guid_configure: FnDeviceGuidConfigure,
    pub request_context_make_shared: FnRequestContextMakeShared,
    pub request_context_config_ctor: FnRequestContextConfigCtor,
    pub request_context_set_base_dir: FnRequestContextConfigSetString,
    pub request_context_set_client_id: FnRequestContextConfigSetString,
    pub request_context_set_version_id: FnRequestContextConfigSetString,
    pub request_context_set_platform_id: FnRequestContextConfigSetString,
    pub request_context_set_product_version: FnRequestContextConfigSetString,
    pub request_context_set_device_model: FnRequestContextConfigSetString,
    pub request_context_set_build_version: FnRequestContextConfigSetString,
    pub request_context_set_locale_id: FnRequestContextConfigSetString,
    pub request_context_set_language_id: FnRequestContextConfigSetString,
    pub request_context_manager_configure: FnRequestContextManagerConfigure,
    pub request_context_init: FnRequestContextInit,
    pub request_context_set_presentation: FnRequestContextSetPresentation,
    pub request_context_set_fairplay_dir: FnRequestContextSetFairPlayDir,
    pub android_presentation_make_shared: FnAndroidPresentationMakeShared,
    pub set_dialog_handler: FnSetDialogHandler,
    pub set_credential_handler: FnSetCredentialHandler,
    pub handle_dialog_response: FnHandleDialogResponse,
    pub handle_credentials_response: FnHandleCredentialsResponse,
    pub protocol_dialog_response_ctor: FnProtocolDialogResponseCtor,
    pub protocol_dialog_response_set_selected_button: FnProtocolDialogResponseSetSelectedButton,
    pub protocol_dialog_title: FnProtocolDialogTitle,
    pub protocol_dialog_message: FnProtocolDialogMessage,
    pub protocol_dialog_buttons: FnProtocolDialogButtons,
    pub protocol_button_title: FnProtocolButtonTitle,
    pub credentials_request_title: FnCredentialsRequestTitle,
    pub credentials_request_message: FnCredentialsRequestMessage,
    pub credentials_request_needs_2fa: FnCredentialsRequestNeeds2fa,
    pub credentials_response_ctor: FnCredentialsResponseCtor,
    pub credentials_response_set_username: FnCredentialsResponseSetString,
    pub credentials_response_set_password: FnCredentialsResponseSetString,
    pub credentials_response_set_type: FnCredentialsResponseSetType,
    pub authenticate_flow_make_shared: FnAuthenticateFlowMakeShared,
    pub authenticate_flow_run: FnAuthenticateFlowRun,
    pub authenticate_flow_response: FnAuthenticateFlowResponse,
    pub authenticate_response_type: FnAuthenticateResponseType,
    pub lease_manager_ctor: FnLeaseManagerCtor,
    pub lease_manager_refresh: FnLeaseManagerRefresh,
    pub lease_manager_request_lease: FnLeaseManagerRequestLease,
    pub lease_manager_release: FnLeaseManagerRelease,
    pub session_ctrl_instance: FnSessionCtrlInstance,
    pub session_ctrl_get_persistent_key: FnSessionCtrlGetPersistentKey,
    pub session_ctrl_decrypt_context: FnSessionCtrlDecryptContext,
    pub pcontext_kd_context: FnPContextKdContext,
    pub shared_ptr_pcontext_drop: FnSharedPtrPContextDrop,
    pub decrypt_sample: FnDecryptSample,
    pub request_context_storefront_identifier: FnRequestContextStorefrontIdentifier,
    pub request_context_fairplay: FnRequestContextFairPlay,
    pub fairplay_get_subscription_status: FnFairPlayGetSubscriptionStatus,
    pub playback_lease_manager_request_asset: FnPlaybackLeaseManagerRequestAsset,
    pub playback_asset_response_has_valid_asset: FnPlaybackAssetResponseHasValidAsset,
    pub playback_asset_response_playback_asset: FnPlaybackAssetResponsePlaybackAsset,
    pub playback_asset_url_string: FnPlaybackAssetUrlString,
    pub http_message_ctor: FnHttpMessageCtor,
    pub http_message_set_header: FnHttpMessageSetHeader,
    pub http_message_set_body_data: FnHttpMessageSetBodyData,
    pub device_guid_guid: FnDeviceGuidGuid,
    pub data_bytes: FnDataBytes,
    pub data_length: FnDataLength,
    pub url_request_ctor: FnUrlRequestCtor,
    pub url_request_set_parameter: FnUrlRequestSetParameter,
    pub url_request_run: FnUrlRequestRun,
    pub url_request_error: FnUrlRequestError,
    pub url_request_response: FnUrlRequestResponse,
    pub url_response_underlying_response: FnUrlResponseUnderlyingResponse,
    pub purchase_request_ctor: FnPurchaseRequestCtor,
    pub purchase_request_set_process_dialog_actions: FnPurchaseRequestSetProcessDialogActions,
    pub purchase_request_set_url_bag_key: FnPurchaseRequestSetString,
    pub purchase_request_set_buy_parameters: FnPurchaseRequestSetString,
    pub purchase_request_run: FnPurchaseRequestRun,
    pub purchase_request_response: FnPurchaseRequestResponse,
    pub purchase_response_error: FnPurchaseResponseError,
    pub purchase_response_items: FnPurchaseResponseItems,
    pub purchase_item_assets: FnPurchaseItemAssets,
    pub purchase_asset_url: FnPurchaseAssetUrl,
    pub protocol_dialog_response_vtable: *mut c_void,
    pub credentials_response_vtable: *mut c_void,
    pub request_context_config_vtable: *mut c_void,
    pub http_message_vtable: *mut c_void,
}

impl NativeSymbols {
    pub fn load(library_dir: &Path) -> AppResult<Self> {
        let flags = libc::RTLD_NOW | libc::RTLD_GLOBAL;
        let preload_libraries = [
            "libc.so",
            "libdl.so",
            "libm.so",
            "libz.so",
            "liblog.so",
            "libandroid.so",
            "libOpenSLES.so",
            "libc++_shared.so",
            "libBlocksRuntime.so",
            "libdispatch.so",
            "libCoreFoundation.so",
            "libicudata_sv_apple.so",
            "libicui18n_sv_apple.so",
            "libxml2.so",
            "libmediaplatform.so",
            "libstoreservicescore.so",
            "libCoreFP.so",
            "libCoreADI.so",
            "libCoreLSKD.so",
            "libdaapkit.so",
            "libmedialibrarycore.so",
            "libandroidappmusic.so",
        ];

        let mut libraries = Vec::with_capacity(preload_libraries.len());
        for name in preload_libraries {
            let path = library_dir.join(name);
            if !path.exists() {
                crate::app_warn!(
                    "ffi::loader",
                    "skipping missing shared library: {}",
                    path.display(),
                );
                continue;
            }
            crate::app_info!(
                "ffi::loader",
                "opening shared library with RTLD_GLOBAL: {}",
                path.display(),
            );
            let library = unsafe { Library::open(Some(&path), flags)? };
            libraries.push(library);
        }
        crate::app_info!(
            "ffi::loader",
            "shared libraries opened successfully: count={}",
            libraries.len(),
        );

        let symbols = Self {
            resolv_set_nameservers: load_symbol!(
                "_resolv_set_nameservers_for_net",
                FnResolvSetNameservers
            ),
            foothill_config: load_symbol!(
                "_ZN14FootHillConfig6configERKNSt6__ndk112basic_stringIcNS0_11char_traitsIcEENS0_9allocatorIcEEEE",
                FnFootHillConfig
            ),
            device_guid_instance: load_symbol!(
                "_ZN17storeservicescore10DeviceGUID8instanceEv",
                FnDeviceGuidInstance
            ),
            device_guid_configure: load_symbol!(
                "_ZN17storeservicescore10DeviceGUID9configureERKNSt6__ndk112basic_stringIcNS1_11char_traitsIcEENS1_9allocatorIcEEEES9_RKjRKb",
                FnDeviceGuidConfigure
            ),
            request_context_make_shared: load_symbol!(
                "_ZNSt6__ndk110shared_ptrIN17storeservicescore14RequestContextEE11make_sharedIJRNS_12basic_stringIcNS_11char_traitsIcEENS_9allocatorIcEEEEEEES3_DpOT_",
                FnRequestContextMakeShared
            ),
            request_context_config_ctor: load_symbol!(
                "_ZN17storeservicescore20RequestContextConfigC2Ev",
                FnRequestContextConfigCtor
            ),
            request_context_set_base_dir: load_symbol!(
                "_ZN17storeservicescore20RequestContextConfig20setBaseDirectoryPathERKNSt6__ndk112basic_stringIcNS1_11char_traitsIcEENS1_9allocatorIcEEEE",
                FnRequestContextConfigSetString
            ),
            request_context_set_client_id: load_symbol!(
                "_ZN17storeservicescore20RequestContextConfig19setClientIdentifierERKNSt6__ndk112basic_stringIcNS1_11char_traitsIcEENS1_9allocatorIcEEEE",
                FnRequestContextConfigSetString
            ),
            request_context_set_version_id: load_symbol!(
                "_ZN17storeservicescore20RequestContextConfig20setVersionIdentifierERKNSt6__ndk112basic_stringIcNS1_11char_traitsIcEENS1_9allocatorIcEEEE",
                FnRequestContextConfigSetString
            ),
            request_context_set_platform_id: load_symbol!(
                "_ZN17storeservicescore20RequestContextConfig21setPlatformIdentifierERKNSt6__ndk112basic_stringIcNS1_11char_traitsIcEENS1_9allocatorIcEEEE",
                FnRequestContextConfigSetString
            ),
            request_context_set_product_version: load_symbol!(
                "_ZN17storeservicescore20RequestContextConfig17setProductVersionERKNSt6__ndk112basic_stringIcNS1_11char_traitsIcEENS1_9allocatorIcEEEE",
                FnRequestContextConfigSetString
            ),
            request_context_set_device_model: load_symbol!(
                "_ZN17storeservicescore20RequestContextConfig14setDeviceModelERKNSt6__ndk112basic_stringIcNS1_11char_traitsIcEENS1_9allocatorIcEEEE",
                FnRequestContextConfigSetString
            ),
            request_context_set_build_version: load_symbol!(
                "_ZN17storeservicescore20RequestContextConfig15setBuildVersionERKNSt6__ndk112basic_stringIcNS1_11char_traitsIcEENS1_9allocatorIcEEEE",
                FnRequestContextConfigSetString
            ),
            request_context_set_locale_id: load_symbol!(
                "_ZN17storeservicescore20RequestContextConfig19setLocaleIdentifierERKNSt6__ndk112basic_stringIcNS1_11char_traitsIcEENS1_9allocatorIcEEEE",
                FnRequestContextConfigSetString
            ),
            request_context_set_language_id: load_symbol!(
                "_ZN17storeservicescore20RequestContextConfig21setLanguageIdentifierERKNSt6__ndk112basic_stringIcNS1_11char_traitsIcEENS1_9allocatorIcEEEE",
                FnRequestContextConfigSetString
            ),
            request_context_manager_configure: load_symbol!(
                "_ZN21RequestContextManager9configureERKNSt6__ndk110shared_ptrIN17storeservicescore14RequestContextEEE",
                FnRequestContextManagerConfigure
            ),
            request_context_init: load_symbol!(
                "_ZN17storeservicescore14RequestContext4initERKNSt6__ndk110shared_ptrINS_20RequestContextConfigEEE",
                FnRequestContextInit
            ),
            request_context_set_presentation: load_symbol!(
                "_ZN17storeservicescore14RequestContext24setPresentationInterfaceERKNSt6__ndk110shared_ptrINS_21PresentationInterfaceEEE",
                FnRequestContextSetPresentation
            ),
            request_context_set_fairplay_dir: load_symbol!(
                "_ZN17storeservicescore14RequestContext24setFairPlayDirectoryPathERKNSt6__ndk112basic_stringIcNS1_11char_traitsIcEENS1_9allocatorIcEEEE",
                FnRequestContextSetFairPlayDir
            ),
            android_presentation_make_shared: load_symbol!(
                "_ZNSt6__ndk110shared_ptrIN20androidstoreservices28AndroidPresentationInterfaceEE11make_sharedIJEEES3_DpOT_",
                FnAndroidPresentationMakeShared
            ),
            set_dialog_handler: load_symbol!(
                "_ZN20androidstoreservices28AndroidPresentationInterface16setDialogHandlerEPFvlNSt6__ndk110shared_ptrIN17storeservicescore14ProtocolDialogEEENS2_INS_36AndroidProtocolDialogResponseHandlerEEEE",
                FnSetDialogHandler
            ),
            set_credential_handler: load_symbol!(
                "_ZN20androidstoreservices28AndroidPresentationInterface21setCredentialsHandlerEPFvNSt6__ndk110shared_ptrIN17storeservicescore18CredentialsRequestEEENS2_INS_33AndroidCredentialsResponseHandlerEEEE",
                FnSetCredentialHandler
            ),
            handle_dialog_response: load_symbol!(
                "_ZN20androidstoreservices28AndroidPresentationInterface28handleProtocolDialogResponseERKlRKNSt6__ndk110shared_ptrIN17storeservicescore22ProtocolDialogResponseEEE",
                FnHandleDialogResponse
            ),
            handle_credentials_response: load_symbol!(
                "_ZN20androidstoreservices28AndroidPresentationInterface25handleCredentialsResponseERKNSt6__ndk110shared_ptrIN17storeservicescore19CredentialsResponseEEE",
                FnHandleCredentialsResponse
            ),
            protocol_dialog_response_ctor: load_symbol!(
                "_ZN17storeservicescore22ProtocolDialogResponseC1Ev",
                FnProtocolDialogResponseCtor
            ),
            protocol_dialog_response_set_selected_button: load_symbol!(
                "_ZN17storeservicescore22ProtocolDialogResponse17setSelectedButtonERKNSt6__ndk110shared_ptrINS_14ProtocolButtonEEE",
                FnProtocolDialogResponseSetSelectedButton
            ),
            protocol_dialog_title: load_symbol!(
                "_ZNK17storeservicescore14ProtocolDialog5titleEv",
                FnProtocolDialogTitle
            ),
            protocol_dialog_message: load_symbol!(
                "_ZNK17storeservicescore14ProtocolDialog7messageEv",
                FnProtocolDialogMessage
            ),
            protocol_dialog_buttons: load_symbol!(
                "_ZNK17storeservicescore14ProtocolDialog7buttonsEv",
                FnProtocolDialogButtons
            ),
            protocol_button_title: load_symbol!(
                "_ZNK17storeservicescore14ProtocolButton5titleEv",
                FnProtocolButtonTitle
            ),
            credentials_request_title: load_symbol!(
                "_ZNK17storeservicescore18CredentialsRequest5titleEv",
                FnCredentialsRequestTitle
            ),
            credentials_request_message: load_symbol!(
                "_ZNK17storeservicescore18CredentialsRequest7messageEv",
                FnCredentialsRequestMessage
            ),
            credentials_request_needs_2fa: load_symbol!(
                "_ZNK17storeservicescore18CredentialsRequest28requiresHSA2VerificationCodeEv",
                FnCredentialsRequestNeeds2fa
            ),
            credentials_response_ctor: load_symbol!(
                "_ZN17storeservicescore19CredentialsResponseC1Ev",
                FnCredentialsResponseCtor
            ),
            credentials_response_set_username: load_symbol!(
                "_ZN17storeservicescore19CredentialsResponse11setUserNameERKNSt6__ndk112basic_stringIcNS1_11char_traitsIcEENS1_9allocatorIcEEEE",
                FnCredentialsResponseSetString
            ),
            credentials_response_set_password: load_symbol!(
                "_ZN17storeservicescore19CredentialsResponse11setPasswordERKNSt6__ndk112basic_stringIcNS1_11char_traitsIcEENS1_9allocatorIcEEEE",
                FnCredentialsResponseSetString
            ),
            credentials_response_set_type: load_symbol!(
                "_ZN17storeservicescore19CredentialsResponse15setResponseTypeENS0_12ResponseTypeE",
                FnCredentialsResponseSetType
            ),
            authenticate_flow_make_shared: load_symbol!(
                "_ZNSt6__ndk110shared_ptrIN17storeservicescore16AuthenticateFlowEE11make_sharedIJRNS0_INS1_14RequestContextEEEEEES3_DpOT_",
                FnAuthenticateFlowMakeShared
            ),
            authenticate_flow_run: load_symbol!(
                "_ZN17storeservicescore16AuthenticateFlow3runEv",
                FnAuthenticateFlowRun
            ),
            authenticate_flow_response: load_symbol!(
                "_ZNK17storeservicescore16AuthenticateFlow8responseEv",
                FnAuthenticateFlowResponse
            ),
            authenticate_response_type: load_symbol!(
                "_ZNK17storeservicescore20AuthenticateResponse12responseTypeEv",
                FnAuthenticateResponseType
            ),
            lease_manager_ctor: load_symbol!(
                "_ZN22SVPlaybackLeaseManagerC2ERKNSt6__ndk18functionIFvRKiEEERKNS1_IFvRKNS0_10shared_ptrIN17storeservicescore19StoreErrorConditionEEEEEE",
                FnLeaseManagerCtor
            ),
            lease_manager_refresh: load_symbol!(
                "_ZN22SVPlaybackLeaseManager25refreshLeaseAutomaticallyERKb",
                FnLeaseManagerRefresh
            ),
            lease_manager_request_lease: load_symbol!(
                "_ZN22SVPlaybackLeaseManager12requestLeaseERKb",
                FnLeaseManagerRequestLease
            ),
            lease_manager_release: load_symbol!(
                "_ZN22SVPlaybackLeaseManager7releaseEv",
                FnLeaseManagerRelease
            ),
            session_ctrl_instance: load_symbol!(
                "_ZN21SVFootHillSessionCtrl8instanceEv",
                FnSessionCtrlInstance
            ),
            session_ctrl_get_persistent_key: load_symbol!(
                "_ZN21SVFootHillSessionCtrl16getPersistentKeyERKNSt6__ndk112basic_stringIcNS0_11char_traitsIcEENS0_9allocatorIcEEEES8_S8_S8_S8_S8_S8_S8_",
                FnSessionCtrlGetPersistentKey
            ),
            session_ctrl_decrypt_context: load_symbol!(
                "_ZN21SVFootHillSessionCtrl14decryptContextERKNSt6__ndk112basic_stringIcNS0_11char_traitsIcEENS0_9allocatorIcEEEERKN11SVDecryptor15SVDecryptorTypeERKb",
                FnSessionCtrlDecryptContext
            ),
            pcontext_kd_context: load_symbol!(
                "_ZNK18SVFootHillPContext9kdContextEv",
                FnPContextKdContext
            ),
            shared_ptr_pcontext_drop: load_symbol!(
                "_ZNSt6__ndk110shared_ptrI18SVFootHillPContextED2Ev",
                FnSharedPtrPContextDrop
            ),
            decrypt_sample: load_symbol!("NfcRKVnxuKZy04KWbdFu71Ou", FnDecryptSample),
            request_context_storefront_identifier: load_symbol!(
                "_ZNK17storeservicescore14RequestContext20storeFrontIdentifierERKNSt6__ndk110shared_ptrINS_6URLBagEEE",
                FnRequestContextStorefrontIdentifier
            ),
            request_context_fairplay: load_symbol!(
                "_ZN17storeservicescore14RequestContext8fairPlayEv",
                FnRequestContextFairPlay
            ),
            fairplay_get_subscription_status: load_symbol!(
                "_ZN17storeservicescore8FairPlay21getSubscriptionStatusEv",
                FnFairPlayGetSubscriptionStatus
            ),
            playback_lease_manager_request_asset: load_symbol!(
                "_ZN22SVPlaybackLeaseManager12requestAssetERKmRKNSt6__ndk16vectorINS2_12basic_stringIcNS2_11char_traitsIcEENS2_9allocatorIcEEEENS7_IS9_EEEERKb",
                FnPlaybackLeaseManagerRequestAsset
            ),
            playback_asset_response_has_valid_asset: load_symbol!(
                "_ZNK23SVPlaybackAssetResponse13hasValidAssetEv",
                FnPlaybackAssetResponseHasValidAsset
            ),
            playback_asset_response_playback_asset: load_symbol!(
                "_ZNK23SVPlaybackAssetResponse13playbackAssetEv",
                FnPlaybackAssetResponsePlaybackAsset
            ),
            playback_asset_url_string: load_symbol!(
                "_ZNK17storeservicescore13PlaybackAsset9URLStringEv",
                FnPlaybackAssetUrlString
            ),
            http_message_ctor: load_symbol!(
                "_ZN13mediaplatform11HTTPMessageC2ENSt6__ndk112basic_stringIcNS1_11char_traitsIcEENS1_9allocatorIcEEEES7_",
                FnHttpMessageCtor
            ),
            http_message_set_header: load_symbol!(
                "_ZN13mediaplatform11HTTPMessage9setHeaderERKNSt6__ndk112basic_stringIcNS1_11char_traitsIcEENS1_9allocatorIcEEEES9_",
                FnHttpMessageSetHeader
            ),
            http_message_set_body_data: load_symbol!(
                "_ZN13mediaplatform11HTTPMessage11setBodyDataEPcm",
                FnHttpMessageSetBodyData
            ),
            device_guid_guid: load_symbol!(
                "_ZN17storeservicescore10DeviceGUID4guidEv",
                FnDeviceGuidGuid
            ),
            data_bytes: load_symbol!("_ZNK13mediaplatform4Data5bytesEv", FnDataBytes),
            data_length: load_symbol!("_ZNK13mediaplatform4Data6lengthEv", FnDataLength),
            url_request_ctor: load_symbol!(
                "_ZN17storeservicescore10URLRequestC2ERKNSt6__ndk110shared_ptrIN13mediaplatform11HTTPMessageEEERKNS2_INS_14RequestContextEEE",
                FnUrlRequestCtor
            ),
            url_request_set_parameter: load_symbol!(
                "_ZN17storeservicescore10URLRequest19setRequestParameterERKNSt6__ndk112basic_stringIcNS1_11char_traitsIcEENS1_9allocatorIcEEEES9_",
                FnUrlRequestSetParameter
            ),
            url_request_run: load_symbol!(
                "_ZN17storeservicescore10URLRequest3runEv",
                FnUrlRequestRun
            ),
            url_request_error: load_symbol!(
                "_ZNK17storeservicescore10URLRequest5errorEv",
                FnUrlRequestError
            ),
            url_request_response: load_symbol!(
                "_ZNK17storeservicescore10URLRequest8responseEv",
                FnUrlRequestResponse
            ),
            url_response_underlying_response: load_symbol!(
                "_ZNK17storeservicescore11URLResponse18underlyingResponseEv",
                FnUrlResponseUnderlyingResponse
            ),
            purchase_request_ctor: load_symbol!(
                "_ZN17storeservicescore15PurchaseRequestC2ERKNSt6__ndk110shared_ptrINS_14RequestContextEEE",
                FnPurchaseRequestCtor
            ),
            purchase_request_set_process_dialog_actions: load_symbol!(
                "_ZN17storeservicescore15PurchaseRequest23setProcessDialogActionsEb",
                FnPurchaseRequestSetProcessDialogActions
            ),
            purchase_request_set_url_bag_key: load_symbol!(
                "_ZN17storeservicescore15PurchaseRequest12setURLBagKeyERKNSt6__ndk112basic_stringIcNS1_11char_traitsIcEENS1_9allocatorIcEEEE",
                FnPurchaseRequestSetString
            ),
            purchase_request_set_buy_parameters: load_symbol!(
                "_ZN17storeservicescore15PurchaseRequest16setBuyParametersERKNSt6__ndk112basic_stringIcNS1_11char_traitsIcEENS1_9allocatorIcEEEE",
                FnPurchaseRequestSetString
            ),
            purchase_request_run: load_symbol!(
                "_ZN17storeservicescore15PurchaseRequest3runEv",
                FnPurchaseRequestRun
            ),
            purchase_request_response: load_symbol!(
                "_ZNK17storeservicescore15PurchaseRequest8responseEv",
                FnPurchaseRequestResponse
            ),
            purchase_response_error: load_symbol!(
                "_ZN17storeservicescore16PurchaseResponse5errorEv",
                FnPurchaseResponseError
            ),
            purchase_response_items: load_symbol!(
                "_ZNK17storeservicescore16PurchaseResponse5itemsEv",
                FnPurchaseResponseItems
            ),
            purchase_item_assets: load_symbol!(
                "_ZNK17storeservicescore12PurchaseItem6assetsEv",
                FnPurchaseItemAssets
            ),
            purchase_asset_url: load_symbol!(
                "_ZNK17storeservicescore13PurchaseAsset3URLEv",
                FnPurchaseAssetUrl
            ),
            protocol_dialog_response_vtable: load_data_symbol!(
                "_ZTVNSt6__ndk120__shared_ptr_emplaceIN17storeservicescore22ProtocolDialogResponseENS_9allocatorIS2_EEEE"
            ),
            credentials_response_vtable: load_data_symbol!(
                "_ZTVNSt6__ndk120__shared_ptr_emplaceIN17storeservicescore19CredentialsResponseENS_9allocatorIS2_EEEE"
            ),
            request_context_config_vtable: load_data_symbol!(
                "_ZTVNSt6__ndk120__shared_ptr_emplaceIN17storeservicescore20RequestContextConfigENS_9allocatorIS2_EEEE"
            ),
            http_message_vtable: load_data_symbol!(
                "_ZTVNSt6__ndk120__shared_ptr_emplaceIN13mediaplatform11HTTPMessageENS_9allocatorIS2_EEEE"
            ),
        };

        Box::leak(Box::new(libraries));
        crate::app_info!(
            "ffi::loader",
            "critical symbols resolved: request_context_make_shared, authenticate_flow_run, session_ctrl_decrypt_context, decrypt_sample"
        );
        Ok(symbols)
    }
}

unsafe impl Send for NativeSymbols {}
unsafe impl Sync for NativeSymbols {}

fn load_global_symbol<T: Copy>(name: &str) -> AppResult<T> {
    let cname = CString::new(name)
        .map_err(|_| AppError::Native(format!("symbol name contains interior NUL: {name}")))?;
    let ptr = unsafe { libc::dlsym(libc::RTLD_DEFAULT, cname.as_ptr()) };
    if ptr.is_null() {
        crate::app_error!(
            "ffi::loader",
            "failed to resolve symbol via RTLD_DEFAULT: {name}"
        );
        return Err(AppError::Native(format!("undefined symbol: {name}")));
    }
    Ok(unsafe { std::mem::transmute_copy(&ptr) })
}
