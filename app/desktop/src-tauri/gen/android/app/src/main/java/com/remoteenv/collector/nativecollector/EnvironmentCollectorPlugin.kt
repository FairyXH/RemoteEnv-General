package com.remoteenv.collector.nativecollector

import android.Manifest
import android.app.Activity
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothManager
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanResult
import android.content.Context
import android.content.ComponentName
import android.text.TextUtils
import android.content.pm.PackageManager
import android.location.GnssStatus
import android.location.Location
import android.location.LocationManager
import android.location.LocationListener
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.wifi.ScanResult as WifiScanResult
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.PowerManager
import android.provider.Settings
import android.content.Intent
import android.net.Uri
import android.telephony.*
import androidx.core.app.ActivityCompat
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import app.tauri.annotation.InvokeArg
import org.json.JSONArray
import org.json.JSONObject
import java.util.Base64
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

@TauriPlugin
class EnvironmentCollectorPlugin(private val host: Activity) : Plugin(host) {
  private fun now() = System.currentTimeMillis()
  private fun allowed(permission: String) =
    ActivityCompat.checkSelfPermission(host, permission) == PackageManager.PERMISSION_GRANTED

  @Command
  fun collectAll(invoke: Invoke) {
    Thread {
      try {
        val response = JSObject()
        response.put("events", collectAllEvents(host.applicationContext))
        invoke.resolve(response)
      } catch (error: Exception) {
        invoke.reject(error.message ?: error.javaClass.simpleName)
      }
    }.start()
  }

  @InvokeArg class ToggleArgs { var enabled: Boolean = false }
  @InvokeArg class AppAuthorizationArgs { var packageName: String = ""; var allowed: Boolean = false }

  @Command
  fun getPersistenceSettings(invoke: Invoke) {
    val preferences = host.getSharedPreferences("collector_persistence", Context.MODE_PRIVATE)
    val power = host.getSystemService(Context.POWER_SERVICE) as PowerManager
    val response = JSObject()
    response.put("foreground_enabled", preferences.getBoolean("foreground_enabled", false))
    response.put("auto_start_enabled", preferences.getBoolean("auto_start_enabled", false))
    response.put("hide_from_recents", preferences.getBoolean("hide_from_recents", false))
    response.put("accessibility_enabled", accessibilityEnabled())
    response.put("battery_optimization_ignored", power.isIgnoringBatteryOptimizations(host.packageName))
    response.put("root_enabled", preferences.getBoolean("root_enabled", false))
    response.put("root_available", RootSupport.available())
    response.put("device_admin_active", DeviceOwnerSupport.isAdmin(host))
    response.put("device_owner_active", DeviceOwnerSupport.isDeviceOwner(host))
    response.put("profile_owner_active", DeviceOwnerSupport.isProfileOwner(host))
    response.put("dhizuku_compat_enabled", DeviceOwnerSupport.compatEnabled(host))
    response.put("dhizuku_supported", DeviceOwnerSupport.isDhizukuSupported())
    response.put("android_api_level", Build.VERSION.SDK_INT)
    response.put("background_location_granted", Build.VERSION.SDK_INT < 29 || allowed(Manifest.permission.ACCESS_BACKGROUND_LOCATION))
    val locationManager = host.getSystemService(Context.LOCATION_SERVICE) as LocationManager
    response.put("location_enabled", if (Build.VERSION.SDK_INT >= 28) locationManager.isLocationEnabled else listOf(LocationManager.GPS_PROVIDER, LocationManager.NETWORK_PROVIDER).any { runCatching { locationManager.isProviderEnabled(it) }.getOrDefault(false) })
    response.put("device_owner_command", DeviceOwnerSupport.adbActivationCommand(host))
    invoke.resolve(response)
  }

  @Command
  fun listDhizukuApps(invoke: Invoke) {
    Thread {
      runCatching {
        val apps = JSONArray()
        host.packageManager.getInstalledPackages(PackageManager.GET_PERMISSIONS)
          .filter { it.packageName != host.packageName && it.requestedPermissions?.contains(DeviceOwnerSupport.API_PERMISSION) == true }
          .sortedBy { it.applicationInfo?.loadLabel(host.packageManager)?.toString()?.lowercase() ?: it.packageName }
          .forEach { pkg ->
            val info = pkg.applicationInfo ?: return@forEach
            apps.put(JSONObject().put("package_name", pkg.packageName).put("label", info.loadLabel(host.packageManager).toString())
              .put("uid", info.uid).put("allowed", DeviceOwnerSupport.isUidAllowed(host, info.uid)))
          }
        val response = JSObject(); response.put("apps", apps); invoke.resolve(response)
      }.onFailure { invoke.reject(it.message ?: it.javaClass.simpleName) }
    }.start()
  }

  @Command
  fun setDhizukuAppAuthorization(invoke: Invoke) {
    runCatching {
      val args = invoke.parseArgs(AppAuthorizationArgs::class.java)
      DeviceOwnerSupport.setAppAllowed(host, args.packageName, args.allowed)
      val response = JSObject(); response.put("allowed", args.allowed); invoke.resolve(response)
    }.onFailure { invoke.reject(it.message ?: it.javaClass.simpleName) }
  }

  @Command
  fun requestDeviceAdmin(invoke: Invoke) {
    runCatching { DeviceOwnerSupport.requestLegacyAdmin(host); invoke.resolve() }
      .onFailure { invoke.reject(it.message) }
  }

  @Command
  fun requestBackgroundLocation(invoke: Invoke) {
    runCatching {
      require(allowed(Manifest.permission.ACCESS_FINE_LOCATION)) { "请先允许精确位置信息权限" }
      when {
        Build.VERSION.SDK_INT < 29 -> Unit
        Build.VERSION.SDK_INT == 29 -> ActivityCompat.requestPermissions(host, arrayOf(Manifest.permission.ACCESS_BACKGROUND_LOCATION), 1002)
        else -> host.startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:${host.packageName}")))
      }
      invoke.resolve()
    }.onFailure { invoke.reject(it.message ?: it.javaClass.simpleName) }
  }

  @Command
  fun setDhizukuCompatEnabled(invoke: Invoke) {
    runCatching {
      val enabled = invoke.parseArgs(ToggleArgs::class.java).enabled
      DeviceOwnerSupport.setCompatEnabled(host, enabled)
      val response = JSObject(); response.put("enabled", enabled); invoke.resolve(response)
    }.onFailure { invoke.reject(it.message) }
  }

  @Command
  fun setForegroundEnabled(invoke: Invoke) {
    runCatching {
      val enabled = invoke.parseArgs(ToggleArgs::class.java).enabled
      CollectorForegroundService.setEnabled(host, enabled)
      val response = JSObject(); response.put("enabled", enabled); invoke.resolve(response)
    }.onFailure { invoke.reject(it.message) }
  }

  @Command
  fun setAutoStartEnabled(invoke: Invoke) {
    runCatching {
      val enabled = invoke.parseArgs(ToggleArgs::class.java).enabled
      val preferences = host.getSharedPreferences("collector_persistence", Context.MODE_PRIVATE)
      preferences.edit().putBoolean("auto_start_enabled", enabled).apply()
      if (enabled) CollectorForegroundService.setEnabled(host, true)
      val response = JSObject(); response.put("enabled", enabled); invoke.resolve(response)
    }.onFailure { invoke.reject(it.message ?: it.javaClass.simpleName) }
  }

  @Command
  fun setHideFromRecents(invoke: Invoke) {
    runCatching {
      val enabled = invoke.parseArgs(ToggleArgs::class.java).enabled
      host.getSharedPreferences("collector_persistence", Context.MODE_PRIVATE).edit().putBoolean("hide_from_recents", enabled).apply()
      (host as? com.remoteenv.collector.MainActivity)?.setHiddenFromRecents(enabled)
      val response = JSObject(); response.put("enabled", enabled); invoke.resolve(response)
    }.onFailure { invoke.reject(it.message ?: it.javaClass.simpleName) }
  }

  @Command
  fun requestAutoStartPermission(invoke: Invoke) {
    runCatching {
      val candidates = listOf(
        ComponentName("com.miui.securitycenter", "com.miui.permcenter.autostart.AutoStartManagementActivity"),
        ComponentName("com.huawei.systemmanager", "com.huawei.systemmanager.startupmgr.ui.StartupNormalAppListActivity"),
        ComponentName("com.oplus.safecenter", "com.oplus.safecenter.startupapp.StartupAppListActivity"),
        ComponentName("com.coloros.safecenter", "com.coloros.safecenter.startupapp.StartupAppListActivity"),
        ComponentName("com.vivo.permissionmanager", "com.vivo.permissionmanager.activity.BgStartUpManagerActivity")
      )
      val intent = candidates.asSequence().map { Intent().setComponent(it).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK) }
        .firstOrNull { host.packageManager.resolveActivity(it, PackageManager.MATCH_DEFAULT_ONLY) != null }
        ?: Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:${host.packageName}"))
      host.startActivity(intent)
      invoke.resolve()
    }.onFailure { invoke.reject(it.message ?: it.javaClass.simpleName) }
  }

  @Command
  fun requestAccessibilityPermission(invoke: Invoke) {
    runCatching {
      host.startActivity(Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS))
      invoke.resolve()
    }.onFailure { invoke.reject(it.message ?: it.javaClass.simpleName) }
  }

  @Command
  fun requestHomeSettings(invoke: Invoke) {
    runCatching {
      host.startActivity(Intent(Settings.ACTION_HOME_SETTINGS))
      invoke.resolve()
    }.onFailure { invoke.reject(it.message ?: it.javaClass.simpleName) }
  }

  private fun accessibilityEnabled(): Boolean {
    if (Settings.Secure.getInt(host.contentResolver, Settings.Secure.ACCESSIBILITY_ENABLED, 0) != 1) return false
    val expected = ComponentName(host, CollectorAccessibilityService::class.java).flattenToString()
    val enabled = Settings.Secure.getString(host.contentResolver, Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES).orEmpty()
    return enabled.split(':').any { TextUtils.equals(it, expected) }
  }

  @Command
  fun requestBatteryOptimizationExemption(invoke: Invoke) {
    runCatching {
      val power = host.getSystemService(Context.POWER_SERVICE) as PowerManager
      if (!power.isIgnoringBatteryOptimizations(host.packageName)) {
        host.startActivity(Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:${host.packageName}")))
      }
      invoke.resolve()
    }.onFailure { invoke.reject(it.message) }
  }

  @Command
  fun setRootEnabled(invoke: Invoke) {
    Thread {
      val enabled = runCatching { invoke.parseArgs(ToggleArgs::class.java).enabled }.getOrDefault(false)
      val result = RootSupport.apply(host, enabled)
      if (result.success) { val response=JSObject(); response.put("enabled", enabled); response.put("message", result.message); invoke.resolve(response) }
      else invoke.reject(result.message)
    }.start()
  }

  companion object {
  private fun now() = System.currentTimeMillis()

  fun collectAllEvents(context: Context): JSONArray = JSONArray().apply {
    put(runCatching { wifiEvent(context) }.getOrNull() ?: emptyWifi())
    put(runCatching { bluetoothEvent(context) }.getOrNull() ?: emptyBluetooth())
    put(runCatching { cellEvent(context) }.getOrNull() ?: emptyCell())
    put(runCatching { gpsEvent(context) }.getOrNull() ?: emptyGps())
    put(runCatching { gnssEvent(context) }.getOrNull() ?: emptyGnss())
  }

  private fun allowed(context: Context, permission: String) =
    ActivityCompat.checkSelfPermission(context, permission) == PackageManager.PERMISSION_GRANTED

  private fun event(type: String, captured: Long, data: JSONObject) = JSONObject()
    .put("data_type", type).put("timestamp_ms", captured).put("data", data)

  private fun emptyWifi(): JSONObject { val time=now(); return event("wifi", time, JSONObject().put("scan_started_at", time).put("scan_finished_at", time).put("interface", JSONObject.NULL).put("is_connected", false).put("gateway", JSONObject.NULL).put("dns_servers", JSONArray()).put("ip_address", JSONObject.NULL).put("networks", JSONArray())) }
  private fun emptyBluetooth(): JSONObject { val time=now(); return event("bluetooth", time, JSONObject().put("scan_started_at", time).put("scan_finished_at", time).put("technology", "unknown").put("is_enabled", JSONObject.NULL).put("devices", JSONArray())) }
  private fun emptyCell(): JSONObject { val time=now(); return event("cell", time, JSONObject().put("observed_at", time).put("network_type", JSONObject.NULL).put("sim_slot", JSONObject.NULL).put("is_connected", JSONObject.NULL).put("registered", JSONObject.NULL).put("serving", JSONObject.NULL).put("neighbors", JSONArray())) }
  private fun emptyGps(): JSONObject { val time=now(); return event("gps", time, JSONObject().put("fix_at", JSONObject.NULL).put("provider", JSONObject.NULL).put("latitude", JSONObject.NULL).put("longitude", JSONObject.NULL).put("altitude_m", JSONObject.NULL).put("accuracy_m", JSONObject.NULL).put("speed_mps", JSONObject.NULL).put("bearing_deg", JSONObject.NULL).put("satellites", JSONObject.NULL).put("fix_quality", "none").put("mocked", JSONObject.NULL).put("points", JSONArray())) }
  private fun emptyGnss(): JSONObject { val time=now(); return event("gnss", time, JSONObject().put("fix_at", JSONObject.NULL).put("constellation", JSONObject.NULL).put("satellites", JSONArray())) }

  private fun wifiEvent(context: Context): JSONObject? {
    if (!allowed(context, Manifest.permission.ACCESS_FINE_LOCATION)) return null
    val manager = context.applicationContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager ?: return null
    val started = now()
    @Suppress("DEPRECATION") try { manager.startScan() } catch (_: Exception) {}
    @Suppress("DEPRECATION") val scans = try { manager.scanResults.orEmpty() } catch (_: SecurityException) { emptyList() }
    val networks = JSONArray()
    scans.forEach { scan ->
      val frequency = scan.frequency.toDouble()
      networks.put(JSONObject()
        .put("ssid", scan.SSID.takeUnless { it.isNullOrEmpty() })
        .put("bssid", scan.BSSID)
        .put("rssi", scan.level.toDouble()).put("signal_dbm", scan.level.toDouble())
        .put("frequency_mhz", frequency).put("channel", channel(scan.frequency))
        .put("band", band(scan.frequency)).put("security", security(scan))
        .put("hidden", scan.SSID.isNullOrEmpty())
        .put("timestamp", if (scan.timestamp > 0) started - ((android.os.SystemClock.elapsedRealtimeNanos() / 1000 - scan.timestamp) / 1000) else started))
    }
    val connectivity = context.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
    val active = connectivity.activeNetwork
    val capabilities = active?.let(connectivity::getNetworkCapabilities)
    val connected = capabilities?.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) == true
    val links = active?.let(connectivity::getLinkProperties)
    val dns = JSONArray(); links?.dnsServers?.forEach { dns.put(it.hostAddress) }
    val ip = links?.linkAddresses?.firstOrNull { !it.address.isLoopbackAddress }?.address?.hostAddress?.substringBefore('%')
    val gateway = links?.routes?.firstOrNull { it.isDefaultRoute }?.gateway?.hostAddress
    val connectionComplete = connected && !gateway.isNullOrBlank() && dns.length() > 0 && !ip.isNullOrBlank()
    val finished = now()
    return event("wifi", finished, JSONObject()
      .put("scan_started_at", started).put("scan_finished_at", finished)
      .put("interface", links?.interfaceName ?: JSONObject.NULL).put("is_connected", connectionComplete)
      .put("gateway", if (connectionComplete) gateway else JSONObject.NULL).put("dns_servers", if (connectionComplete) dns else JSONArray())
      .put("ip_address", if (connectionComplete) ip else JSONObject.NULL).put("networks", networks))
  }

  private fun channel(f: Int): Int? = when (f) {
    2484 -> 14
    in 2412..2472 -> (f - 2407) / 5
    in 5000..5895 -> (f - 5000) / 5
    in 5955..7115 -> (f - 5950) / 5
    else -> null
  }
  private fun band(f: Int) = when (f) { in 2400..2500 -> "2_4ghz"; in 4900..5924 -> "5ghz"; in 5925..7125 -> "6ghz"; else -> "unknown" }
  private fun security(scan: WifiScanResult): JSONArray {
    val text = scan.capabilities.uppercase(); val values = linkedSetOf<String>()
    if ("SAE" in text) values += "WPA3-SAE"
    if ("WPA2" in text || "RSN" in text) values += "WPA2-PSK"
    if ("WPA-" in text || "WPA]" in text) values += "WPA-PSK"
    if ("EAP" in text) values += "EAP"
    if ("WEP" in text) values += "WEP"
    if (values.isEmpty() && !text.contains("WEP") && !text.contains("WPA") && !text.contains("RSN")) values += "OPEN"
    return JSONArray(values.toList())
  }

  private fun bluetoothEvent(context: Context): JSONObject? {
    val manager = context.getSystemService(Context.BLUETOOTH_SERVICE) as? BluetoothManager ?: return null
    val adapter = manager.adapter ?: return event("bluetooth", now(), JSONObject().put("technology", "unknown").put("is_enabled", false).put("devices", JSONArray()))
    if (Build.VERSION.SDK_INT >= 31 && (!allowed(context, Manifest.permission.BLUETOOTH_SCAN) || !allowed(context, Manifest.permission.BLUETOOTH_CONNECT))) return null
    val started = now(); val found = linkedMapOf<String, JSONObject>(); val latch = CountDownLatch(1)
    val callback = object : ScanCallback() {
      override fun onScanResult(callbackType: Int, result: ScanResult) { found[result.device.address] = bleDevice(result) }
      override fun onBatchScanResults(results: MutableList<ScanResult>) { results.forEach { found[it.device.address] = bleDevice(it) } }
      override fun onScanFailed(errorCode: Int) { latch.countDown() }
    }
    if (adapter.isEnabled) try {
      adapter.bluetoothLeScanner?.startScan(callback); latch.await(4, TimeUnit.SECONDS); adapter.bluetoothLeScanner?.stopScan(callback)
    } catch (_: Exception) {}
    try { adapter.bondedDevices.orEmpty().forEach { device ->
      found.putIfAbsent(device.address, JSONObject().put("address", device.address).put("address_type", "unknown").put("mode", "classic")
        .put("name", device.name).put("is_connected", JSONObject.NULL).put("is_paired", true)
        .put("rssi", JSONObject.NULL).put("tx_power", JSONObject.NULL).put("manufacturer_id", JSONObject.NULL)
        .put("manufacturer_data", JSONObject.NULL).put("service_uuids", JSONArray()).put("service_data", JSONObject())
        .put("raw", JSONObject.NULL).put("rawHex", JSONObject.NULL).put("rawLength", JSONObject.NULL).put("timestamp", now()))
    }} catch (_: SecurityException) {}
    val finished = now(); return event("bluetooth", finished, JSONObject().put("scan_started_at", started)
      .put("scan_finished_at", finished).put("technology", "unknown").put("is_enabled", adapter.isEnabled)
      .put("devices", JSONArray(found.values.toList())))
  }

  private fun bleDevice(result: ScanResult): JSONObject {
    val record = result.scanRecord; val bytes = record?.bytes
    val uuids = JSONArray(); record?.serviceUuids.orEmpty().forEach { uuids.put(it.uuid.toString().lowercase()) }
    val serviceData = JSONObject(); record?.serviceData?.forEach { (key, value) -> serviceData.put(key.uuid.toString().lowercase(), Base64.getEncoder().encodeToString(value)) }
    var manufacturerId: Int? = null; var manufacturer: String? = null
    record?.manufacturerSpecificData?.let { values -> if (values.size() > 0) { manufacturerId = values.keyAt(0); manufacturer = Base64.getEncoder().encodeToString(values.valueAt(0)) } }
    return JSONObject().put("address", result.device.address).put("address_type", "unknown").put("mode", "ble")
      .put("name", record?.deviceName).put("is_connected", JSONObject.NULL)
      .put("is_paired", try { result.device.bondState == android.bluetooth.BluetoothDevice.BOND_BONDED } catch (_: SecurityException) { null })
      .put("rssi", result.rssi.toDouble()).put("tx_power", record?.txPowerLevel?.takeUnless { it == Int.MIN_VALUE }?.toDouble())
      .put("manufacturer_id", manufacturerId).put("manufacturer_data", manufacturer)
      .put("service_uuids", uuids).put("service_data", serviceData)
      .put("raw", bytes?.let { Base64.getEncoder().encodeToString(it) }).put("rawHex", bytes?.joinToString("") { "%02X".format(it) })
      .put("rawLength", bytes?.size).put("timestamp", now())
  }

  private fun cellEvent(context: Context): JSONObject? {
    if (!allowed(context, Manifest.permission.ACCESS_FINE_LOCATION)) return null
    val telephony = context.getSystemService(Context.TELEPHONY_SERVICE) as? TelephonyManager ?: return null
    val cells = try { telephony.allCellInfo.orEmpty() } catch (_: SecurityException) { emptyList() }
    val neighbors = JSONArray(); var serving: JSONObject? = null
    cells.forEach { info -> val item = cell(info); if (info.isRegistered && serving == null) serving = item else neighbors.put(item) }
    val time = now(); return event("cell", time, JSONObject().put("observed_at", time)
      .put("network_type", networkType(telephony)).put("sim_slot", 0)
      .put("is_connected", serving != null).put("registered", cells.any { it.isRegistered })
      .put("serving", serving).put("neighbors", neighbors))
  }

  private fun cell(info: CellInfo): JSONObject {
    val out = JSONObject(); out.put("technology", when(info) { is CellInfoLte -> "lte"; is CellInfoNr -> "nr"; is CellInfoGsm -> "gsm"; is CellInfoWcdma -> "wcdma"; is CellInfoCdma -> "cdma"; else -> "unknown" })
    when (info) {
      is CellInfoLte -> { val i=info.cellIdentity; val s=info.cellSignalStrength; out.put("mcc", i.mccString).put("mnc", i.mncString).put("tac", valid(i.tac)).put("cid", valid(i.ci)).put("pci", valid(i.pci)).put("arfcn", valid(i.earfcn)).put("rssi", s.rssi.toDouble()).put("signal_dbm", s.dbm.toDouble()).put("rsrp", s.rsrp.toDouble()).put("rsrq", s.rsrq.toDouble()).put("asu", s.asuLevel.toDouble()) }
      is CellInfoGsm -> { val i=info.cellIdentity; val s=info.cellSignalStrength; out.put("mcc", i.mccString).put("mnc", i.mncString).put("lac", valid(i.lac)).put("cid", valid(i.cid)).put("arfcn", valid(i.arfcn)).put("rssi", s.dbm.toDouble()).put("signal_dbm", s.dbm.toDouble()).put("asu", s.asuLevel.toDouble()) }
      is CellInfoWcdma -> { val i=info.cellIdentity; val s=info.cellSignalStrength; out.put("mcc", i.mccString).put("mnc", i.mncString).put("lac", valid(i.lac)).put("cid", valid(i.cid)).put("pci", valid(i.psc)).put("arfcn", valid(i.uarfcn)).put("rssi", s.dbm.toDouble()).put("signal_dbm", s.dbm.toDouble()).put("asu", s.asuLevel.toDouble()) }
      is CellInfoCdma -> { val i=info.cellIdentity; val s=info.cellSignalStrength; out.put("lac", valid(i.networkId)).put("cid", valid(i.basestationId)).put("rssi", s.dbm.toDouble()).put("signal_dbm", s.dbm.toDouble()).put("asu", s.asuLevel.toDouble()) }
      is CellInfoNr -> if (Build.VERSION.SDK_INT >= 29) { val i=info.cellIdentity as CellIdentityNr; val s=info.cellSignalStrength as CellSignalStrengthNr; out.put("mcc", i.mccString).put("mnc", i.mncString).put("tac", valid(i.tac)).put("cid", valid(i.nci)).put("pci", valid(i.pci)).put("arfcn", valid(i.nrarfcn)).put("signal_dbm", s.dbm.toDouble()).put("rsrp", s.ssRsrp.toDouble()).put("rsrq", s.ssRsrq.toDouble()).put("asu", s.asuLevel.toDouble()) }
    }; return out
  }
  private fun valid(value: Int): Any? = value.takeUnless { it == Int.MAX_VALUE || it < 0 }
  private fun valid(value: Long): Any? = value.takeUnless { it == Long.MAX_VALUE || it < 0 }
  @Suppress("DEPRECATION") private fun networkType(tm: TelephonyManager) = try {
    when (tm.dataNetworkType) {
      TelephonyManager.NETWORK_TYPE_GPRS -> "gprs"
      TelephonyManager.NETWORK_TYPE_EDGE -> "edge"
      TelephonyManager.NETWORK_TYPE_UMTS -> "umts"
      TelephonyManager.NETWORK_TYPE_CDMA -> "cdma"
      TelephonyManager.NETWORK_TYPE_EVDO_0, TelephonyManager.NETWORK_TYPE_EVDO_A, TelephonyManager.NETWORK_TYPE_EVDO_B -> "evdo"
      TelephonyManager.NETWORK_TYPE_1xRTT -> "1xrtt"
      TelephonyManager.NETWORK_TYPE_HSDPA, TelephonyManager.NETWORK_TYPE_HSUPA, TelephonyManager.NETWORK_TYPE_HSPA, TelephonyManager.NETWORK_TYPE_HSPAP -> "hspa"
      TelephonyManager.NETWORK_TYPE_IDEN -> "iden"
      TelephonyManager.NETWORK_TYPE_LTE -> "lte"
      TelephonyManager.NETWORK_TYPE_EHRPD -> "ehrpd"
      TelephonyManager.NETWORK_TYPE_GSM -> "gsm"
      TelephonyManager.NETWORK_TYPE_TD_SCDMA -> "td-scdma"
      TelephonyManager.NETWORK_TYPE_IWLAN -> "iwlan"
      TelephonyManager.NETWORK_TYPE_NR -> "nr"
      else -> "unknown"
    }
  } catch (_: Exception) { "unknown" }

  private fun gpsEvent(context: Context): JSONObject? {
    if (!allowed(context, Manifest.permission.ACCESS_FINE_LOCATION)) return null
    val manager = context.getSystemService(Context.LOCATION_SERVICE) as LocationManager
    val fixes = java.util.Collections.synchronizedList(mutableListOf<Location>())
    val latch = CountDownLatch(1)
    val listener = LocationListener { location -> fixes.add(location); latch.countDown() }
    val providers = listOf(LocationManager.GPS_PROVIDER, LocationManager.NETWORK_PROVIDER).filter { runCatching { manager.isProviderEnabled(it) }.getOrDefault(false) }
    providers.forEach { provider -> runCatching { manager.requestSingleUpdate(provider, listener, Looper.getMainLooper()) } }
    latch.await(4, TimeUnit.SECONDS)
    runCatching { manager.removeUpdates(listener) }
    val location = (fixes + providers.mapNotNull { try { manager.getLastKnownLocation(it) } catch (_: Exception) { null } }).maxByOrNull(Location::getTime)
    val data = JSONObject().put("fix_at", location?.time).put("provider", location?.provider)
      .put("latitude", location?.latitude).put("longitude", location?.longitude).put("altitude_m", location?.takeIf { it.hasAltitude() }?.altitude)
      .put("accuracy_m", location?.takeIf { it.hasAccuracy() }?.accuracy?.toDouble()).put("speed_mps", location?.takeIf { it.hasSpeed() }?.speed?.toDouble())
      .put("bearing_deg", location?.takeIf { it.hasBearing() }?.bearing?.toDouble()).put("satellites", location?.extras?.getInt("satellites"))
      .put("fix_quality", if (location == null) "none" else "fix").put("mocked", location?.isFromMockProvider).put("points", JSONArray())
    return event("gps", now(), data)
  }

  private fun gnssEvent(context: Context): JSONObject? {
    if (!allowed(context, Manifest.permission.ACCESS_FINE_LOCATION)) return null
    val manager = context.getSystemService(Context.LOCATION_SERVICE) as LocationManager
    val satellites = JSONArray(); val latch = CountDownLatch(1)
    val locationListener = LocationListener { }
    val callback = object : GnssStatus.Callback() { override fun onSatelliteStatusChanged(status: GnssStatus) {
      for (i in 0 until status.satelliteCount) satellites.put(JSONObject().put("type", constellation(status.getConstellationType(i)))
        .put("svid", status.getSvid(i)).put("azimuth_deg", status.getAzimuthDegrees(i).toDouble()).put("elevation_deg", status.getElevationDegrees(i).toDouble())
        .put("frequency_mhz", if (Build.VERSION.SDK_INT >= 26 && status.hasCarrierFrequencyHz(i)) status.getCarrierFrequencyHz(i).toDouble()/1_000_000.0 else null)
        .put("snr_db", status.getCn0DbHz(i).toDouble()).put("almanac", status.hasAlmanacData(i)).put("ephemeris", status.hasEphemerisData(i)).put("used", status.usedInFix(i)))
      latch.countDown()
    }}
    try {
      manager.registerGnssStatusCallback(callback, Handler(Looper.getMainLooper()))
      if (manager.isProviderEnabled(LocationManager.GPS_PROVIDER)) manager.requestLocationUpdates(LocationManager.GPS_PROVIDER, 500L, 0f, locationListener, Looper.getMainLooper())
      latch.await(6, TimeUnit.SECONDS)
      manager.removeUpdates(locationListener)
      manager.unregisterGnssStatusCallback(callback)
    } catch (_: Exception) {
      runCatching { manager.removeUpdates(locationListener) }
      runCatching { manager.unregisterGnssStatusCallback(callback) }
    }
    val time=now(); return event("gnss", time, JSONObject().put("fix_at", time).put("constellation", "mixed").put("satellites", satellites))
  }
  private fun constellation(value: Int) = when(value) { GnssStatus.CONSTELLATION_GPS -> "gps"; GnssStatus.CONSTELLATION_GLONASS -> "glonass"; GnssStatus.CONSTELLATION_GALILEO -> "galileo"; GnssStatus.CONSTELLATION_BEIDOU -> "beidou"; GnssStatus.CONSTELLATION_QZSS -> "qzss"; GnssStatus.CONSTELLATION_SBAS -> "sbas"; else -> "unknown" }
  }
}
