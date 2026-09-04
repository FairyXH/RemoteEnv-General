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
import android.os.Handler
import android.os.Looper
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import com.remoteenv.collector.MainActivity
import com.remoteenv.collector.R

class CollectorForegroundService : Service() {
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
    Handler(Looper.getMainLooper()).post {
      runCatching { MainActivity.ensureBackgroundRuntime(applicationContext) }
    }
    return START_STICKY
  }

  override fun onTaskRemoved(rootIntent: Intent?) {
    val preferences = getSharedPreferences("collector_persistence", Context.MODE_PRIVATE)
    if (preferences.getBoolean("foreground_enabled", false) || preferences.getBoolean("auto_start_enabled", false)) {
      val restart = PendingIntent.getService(this, 1003, Intent(this, CollectorForegroundService::class.java), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
      val alarm = getSystemService(Context.ALARM_SERVICE) as AlarmManager
      alarm.setAndAllowWhileIdle(AlarmManager.ELAPSED_REALTIME_WAKEUP, android.os.SystemClock.elapsedRealtime() + 5_000, restart)
    }
    super.onTaskRemoved(rootIntent)
  }

  companion object {
    private const val CHANNEL = "remote_env_collection"
    private const val NOTIFICATION_ID = 1002
    fun setEnabled(context: Context, enabled: Boolean) {
      val intent = Intent(context, CollectorForegroundService::class.java)
      if (enabled) ContextCompat.startForegroundService(context, intent) else context.stopService(intent)
      context.getSharedPreferences("collector_persistence", Context.MODE_PRIVATE).edit().putBoolean("foreground_enabled", enabled).apply()
    }
  }
}
