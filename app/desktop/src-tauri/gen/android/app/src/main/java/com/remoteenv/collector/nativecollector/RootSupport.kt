package com.remoteenv.collector.nativecollector

import android.content.Context
import java.io.File
import java.util.concurrent.TimeUnit
import java.util.concurrent.Executors

object RootSupport {
  data class Result(val success: Boolean, val message: String)
  private val protector = Executors.newSingleThreadScheduledExecutor { task ->
    Thread(task, "remote-env-root-protector").apply { isDaemon = true }
  }
  @Volatile private var protectionStarted = false

  fun available(): Boolean = sequenceOf("/system/bin/su", "/system/xbin/su", "/sbin/su", "/debug_ramdisk/su")
    .map(::File).any { it.exists() && it.canExecute() } || System.getenv("PATH").orEmpty().split(File.pathSeparator).any { File(it, "su").canExecute() }

  fun apply(context: Context, enabled: Boolean): Result {
    val packageName = context.packageName
    val commands = if (enabled) {
      protectionCommands(packageName)
    } else {
      "for p in \$(pidof $packageName 2>/dev/null); do echo 0 > /proc/\$p/oom_score_adj 2>/dev/null; done; dumpsys deviceidle whitelist -$packageName"
    }
    val result = run(commands, 8)
    if (result.success) {
      context.getSharedPreferences("collector_persistence", Context.MODE_PRIVATE).edit().putBoolean("root_enabled", enabled).apply()
      if (enabled) startProtection(context.applicationContext)
    }
    return result
  }

  fun startProtection(context: Context) {
    if (protectionStarted) return
    synchronized(this) {
      if (protectionStarted) return
      protectionStarted = true
      protector.scheduleWithFixedDelay({
        val enabled = context.getSharedPreferences("collector_persistence", Context.MODE_PRIVATE).getBoolean("root_enabled", false)
        if (enabled) run(protectionCommands(context.packageName), 8)
      }, 0, 30, TimeUnit.SECONDS)
    }
  }

  private fun protectionCommands(packageName: String) = """
    for p in ${'$'}(pidof $packageName 2>/dev/null); do echo -1000 > /proc/${'$'}p/oom_score_adj 2>/dev/null; done
    dumpsys deviceidle whitelist +$packageName >/dev/null 2>&1
    cmd appops set $packageName RUN_IN_BACKGROUND allow >/dev/null 2>&1
    cmd appops set $packageName RUN_ANY_IN_BACKGROUND allow >/dev/null 2>&1
    accessibility_service="$packageName/$packageName.nativecollector.CollectorAccessibilityService"
    accessibility_service_short="$packageName/.nativecollector.CollectorAccessibilityService"
    enabled_services=${'$'}(settings get secure enabled_accessibility_services 2>/dev/null)
    [ "${'$'}enabled_services" = "null" ] && enabled_services=
    case ":${'$'}enabled_services:" in
      *":${'$'}accessibility_service:"*|*":${'$'}accessibility_service_short:"*) ;;
      *)
        if [ -n "${'$'}enabled_services" ]; then
          enabled_services="${'$'}enabled_services:${'$'}accessibility_service"
        else
          enabled_services="${'$'}accessibility_service"
        fi
        settings put secure enabled_accessibility_services "${'$'}enabled_services" >/dev/null 2>&1
        ;;
    esac
    settings put secure accessibility_enabled 1 >/dev/null 2>&1
    pm grant $packageName android.permission.ACCESS_FINE_LOCATION >/dev/null 2>&1
    pm grant $packageName android.permission.ACCESS_COARSE_LOCATION >/dev/null 2>&1
    pm grant $packageName android.permission.ACCESS_BACKGROUND_LOCATION >/dev/null 2>&1
    pm grant $packageName android.permission.READ_PHONE_STATE >/dev/null 2>&1
    pm grant $packageName android.permission.BLUETOOTH_SCAN >/dev/null 2>&1
    pm grant $packageName android.permission.BLUETOOTH_CONNECT >/dev/null 2>&1
    pm grant $packageName android.permission.NEARBY_WIFI_DEVICES >/dev/null 2>&1
    pm grant $packageName android.permission.POST_NOTIFICATIONS >/dev/null 2>&1
  """.trimIndent()

  private fun run(command: String, timeoutSeconds: Long): Result = try {
    val process = ProcessBuilder("su", "-c", command).redirectErrorStream(true).start()
    val finished = process.waitFor(timeoutSeconds, TimeUnit.SECONDS)
    if (!finished) { process.destroyForcibly(); Result(false, "Root 命令超时") }
    else Result(process.exitValue() == 0, process.inputStream.bufferedReader().readText().trim().ifBlank { if (process.exitValue() == 0) "已应用" else "Root 命令失败" })
  } catch (error: Exception) { Result(false, error.message ?: "未获得 Root 授权") }
}
