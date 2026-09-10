package com.remoteenv.collector.nativecollector

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.app.AlarmManager
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.IBinder
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import com.remoteenv.collector.MainActivity
import com.remoteenv.collector.R
import org.json.JSONObject
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit

class CollectorForegroundService : Service() {
  private val collectorExecutor = Executors.newSingleThreadScheduledExecutor { task ->
    Thread(task, "remote-env-headless-collector").apply { isDaemon = true }
  }
  @Volatile private var collectionTask: ScheduledFuture<*>? = null

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onCreate() {
    super.onCreate()
    RootSupport.startProtection(applicationContext)
    if (Build.VERSION.SDK_INT >= 26) {
      val channel = NotificationChannel(CHANNEL, "环境采集服务", NotificationManager.IMPORTANCE_LOW)
      channel.description = "保持环境数据采集与上传稳定运行"
      getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
    }
    val open = PendingIntent.getActivity(this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    val notification = NotificationCompat.Builder(this, CHANNEL)
      .setSmallIcon(R.mipmap.ic_launcher)
      .setContentTitle("远程环境采集器正在运行")
      .setContentText("持续采集并上传已授权的环境数据")
      .setOngoing(true).setOnlyAlertOnce(true).setContentIntent(open).build()
    startForeground(NOTIFICATION_ID, notification)
  }

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    if (!isMasterEnabled(applicationContext)) {
      stopSelf()
      return START_NOT_STICKY
    }
    startHeadlessRuntime()
    return START_STICKY
  }

  private fun startHeadlessRuntime() {
    if (collectionTask?.isCancelled == false && collectionTask?.isDone == false) return
    collectorExecutor.execute {
      Log.i(TAG, "Starting headless runtime from dataDir=${applicationContext.dataDir.absolutePath}")
      val result = runCatching { HeadlessRuntime.start(applicationContext) }
        .getOrElse { "{\"error\":${JSONObject.quote(it.message ?: it.javaClass.simpleName)}}" }
      val error = runCatching { JSONObject(result).optString("error").takeIf(String::isNotBlank) }.getOrNull()
      if (error != null) {
        Log.e(TAG, "Headless runtime start failed: $error")
        return@execute
      }
      scheduleCollection(0)
    }
  }

  private fun scheduleCollection(delayMillis: Long) {
    collectionTask = collectorExecutor.schedule({
      if (!isMasterEnabled(applicationContext)) {
        stopSelf()
        return@schedule
      }
      try {
        val events = EnvironmentCollectorPlugin.collectAllEvents(applicationContext)
        val result = HeadlessRuntime.nativeSubmitEvents(events.toString())
        if (result != "ok") Log.e(TAG, "Headless event submit failed: $result")
      } catch (error: Exception) {
        Log.e(TAG, "Headless collection failed", error)
      } finally {
        val next = runCatching { HeadlessRuntime.nativeIntervalMillis() }
          .getOrDefault(30_000L).coerceAtLeast(1_000L)
        scheduleCollection(next)
      }
    }, delayMillis, TimeUnit.MILLISECONDS)
  }

  override fun onTaskRemoved(rootIntent: Intent?) {
    val preferences = getSharedPreferences("collector_persistence", Context.MODE_PRIVATE)
    if (isMasterEnabled(applicationContext) && (preferences.getBoolean("foreground_enabled", false) || preferences.getBoolean("auto_start_enabled", false))) {
      val restart = PendingIntent.getService(this, 1003, Intent(this, CollectorForegroundService::class.java), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
      val alarm = getSystemService(Context.ALARM_SERVICE) as AlarmManager
      alarm.setAndAllowWhileIdle(AlarmManager.ELAPSED_REALTIME_WAKEUP, android.os.SystemClock.elapsedRealtime() + 5_000, restart)
    }
    super.onTaskRemoved(rootIntent)
  }

  override fun onDestroy() {
    collectionTask?.cancel(true)
    collectorExecutor.shutdownNow()
    runCatching { HeadlessRuntime.nativeStop() }
    super.onDestroy()
  }

  companion object {
    private const val CHANNEL = "remote_env_collection"
    private const val NOTIFICATION_ID = 1002
    private const val TAG = "RemoteEnvCollector"
    private const val PREFS = "collector_persistence"
    private const val MASTER_ENABLED = "master_enabled"

    fun isMasterEnabled(context: Context): Boolean =
      context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean(MASTER_ENABLED, true)

    fun setMasterEnabled(context: Context, enabled: Boolean) {
      context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putBoolean(MASTER_ENABLED, enabled).commit()
      if (!enabled) {
        context.stopService(Intent(context, CollectorForegroundService::class.java))
      }
    }

    fun setEnabled(context: Context, enabled: Boolean) {
      val intent = Intent(context, CollectorForegroundService::class.java)
      if (enabled && isMasterEnabled(context)) ContextCompat.startForegroundService(context, intent) else context.stopService(intent)
      context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putBoolean("foreground_enabled", enabled).apply()
    }
  }
}
