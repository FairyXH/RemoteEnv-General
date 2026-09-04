package com.remoteenv.collector.nativecollector

import android.app.Activity
import android.app.admin.DeviceAdminReceiver
import android.app.admin.DevicePolicyManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.os.Binder
import android.os.Build
import android.content.pm.PackageManager
import java.security.MessageDigest
import com.rosan.dhizuku.aidl.IDhizukuClient
import com.rosan.dhizuku.server_api.DhizukuProvider
import com.rosan.dhizuku.server_api.DhizukuService

class CollectorDeviceAdminReceiver : DeviceAdminReceiver()

object DeviceOwnerSupport {
  private const val PREFS = "collector_persistence"
  private const val COMPAT = "dhizuku_compat_enabled"
  private const val ALLOW_PREFIX = "dhizuku_allow_"
  const val API_PERMISSION = "com.rosan.dhizuku.permission.API"

  fun isDhizukuSupported() = Build.VERSION.SDK_INT in 26..37

  fun admin(context: Context) = ComponentName(context, CollectorDeviceAdminReceiver::class.java)

  fun manager(context: Context) =
    context.getSystemService(Context.DEVICE_POLICY_SERVICE) as DevicePolicyManager

  fun isAdmin(context: Context) = manager(context).isAdminActive(admin(context))
  fun isDeviceOwner(context: Context) = manager(context).isDeviceOwnerApp(context.packageName)
  fun isProfileOwner(context: Context) = manager(context).isProfileOwnerApp(context.packageName)
  fun isOwner(context: Context) = isDeviceOwner(context) || isProfileOwner(context)

  fun compatEnabled(context: Context) =
    isDhizukuSupported() && context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean(COMPAT, false) && isOwner(context)

  fun setCompatEnabled(context: Context, enabled: Boolean) {
    require(!enabled || isDhizukuSupported()) { "当前 Android 版本不在 Dhizuku 2.12.0 支持范围内（Android 8-17）" }
    require(!enabled || isOwner(context)) { "本应用尚未成为 Device Owner 或 Profile Owner" }
    context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putBoolean(COMPAT, enabled).apply()
  }

  fun signature(context: Context, packageName: String): String? = runCatching {
    val flags = if (Build.VERSION.SDK_INT >= 28) PackageManager.GET_SIGNING_CERTIFICATES else @Suppress("DEPRECATION") PackageManager.GET_SIGNATURES
    val info = context.packageManager.getPackageInfo(packageName, flags)
    val bytes = if (Build.VERSION.SDK_INT >= 28) info.signingInfo?.apkContentsSigners?.firstOrNull()?.toByteArray() else @Suppress("DEPRECATION") info.signatures?.firstOrNull()?.toByteArray()
    bytes?.let { MessageDigest.getInstance("SHA-256").digest(it).joinToString("") { byte -> "%02x".format(byte) } }
  }.getOrNull()

  fun setAppAllowed(context: Context, packageName: String, allowed: Boolean) {
    val info = context.packageManager.getApplicationInfo(packageName, 0)
    val signature = signature(context, packageName) ?: error("无法读取应用签名")
    context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
      .putString("$ALLOW_PREFIX${info.uid}", if (allowed) "$packageName|$signature" else null).apply()
    if (!allowed && isOwner(context)) runCatching { manager(context).setDelegatedScopes(admin(context), packageName, emptyList()) }
  }

  fun isUidAllowed(context: Context, uid: Int): Boolean {
    val saved = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString("$ALLOW_PREFIX$uid", null) ?: return false
    return context.packageManager.getPackagesForUid(uid).orEmpty().any { pkg -> saved == "$pkg|${signature(context, pkg)}" }
  }

  fun requestLegacyAdmin(activity: Activity) {
    activity.startActivity(Intent(DevicePolicyManager.ACTION_ADD_DEVICE_ADMIN).apply {
      putExtra(DevicePolicyManager.EXTRA_DEVICE_ADMIN, admin(activity))
      putExtra(DevicePolicyManager.EXTRA_ADD_EXPLANATION, "设备管理员只用于注册接收器；完整后台能力和 Dhizuku 兼容服务需要 Device Owner。")
    })
  }

  fun adbActivationCommand(context: Context) =
    "adb shell dpm set-device-owner ${context.packageName}/${CollectorDeviceAdminReceiver::class.java.name}"
}

class CollectorDhizukuProvider : DhizukuProvider() {
  override fun onCreateService(client: IDhizukuClient): DhizukuService {
    val appContext = requireNotNull(context).applicationContext
    check(DeviceOwnerSupport.compatEnabled(appContext)) { "Dhizuku compatibility service is disabled" }
    return CollectorDhizukuService(appContext, DeviceOwnerSupport.admin(appContext), client)
  }
}

private class CollectorDhizukuService(
  context: Context,
  admin: ComponentName,
  client: IDhizukuClient
) : DhizukuService(context, admin, client) {
  override fun getVersionName() = "RemoteEnvCollector Dhizuku Compatibility 1.0"

  override fun checkCallingPermission(func: String?, callingUid: Int, callingPid: Int): Boolean {
    if (!DeviceOwnerSupport.compatEnabled(mContext)) return false
    if (callingUid == Binder.getCallingUid() && callingUid == android.os.Process.myUid()) return true
    return DeviceOwnerSupport.isUidAllowed(mContext, callingUid)
  }
}
