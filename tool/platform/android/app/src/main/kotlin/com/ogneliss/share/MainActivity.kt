package com.ogneliss.share

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.OpenableColumns
import android.provider.Settings
import android.util.Log
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import java.io.File
import java.io.FileOutputStream
import java.util.UUID

/**
 * Единственная активность приложения.
 *
 * Кроме обычного запуска Flutter здесь:
 *  - ядру на Rust передаются JavaVM и контекст приложения (nativeInit) —
 *    так оно узнаёт DNS-серверы текущей сети;
 *  - на время работы на экране держится MulticastLock — без него многие
 *    телефоны не принимают mDNS и не видят свои устройства в Wi-Fi без интернета;
 *  - принимается «Поделиться» из других приложений: файлы копируются в
 *    личный кэш под безопасными именами, и интерфейс предлагает выбрать,
 *    на какое устройство их отправить;
 *  - открываются ссылки-приглашения (ogshare://pair?… и
 *    https://ogneliss.ru/share/join/#…) — их разбирает ядро;
 *  - текст ссылки отправляется через системное «Поделиться».
 */
class MainActivity : FlutterActivity() {
    private var channel: MethodChannel? = null
    private var multicastLock: WifiManager.MulticastLock? = null
    private var pendingShare: HashMap<String, Any?>? = null
    private var pendingLink: String? = null

    private external fun nativeInit(context: Context)

    override fun onCreate(savedInstanceState: Bundle?) {
        initCore()
        super.onCreate(savedInstanceState)
        if (savedInstanceState == null) {
            handleShare(intent)
            handleLink(intent)
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleShare(intent)
        handleLink(intent)
    }

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        val ch = MethodChannel(flutterEngine.dartExecutor.binaryMessenger, CHANNEL)
        ch.setMethodCallHandler { call, result ->
            when (call.method) {
                "takeShared" -> {
                    result.success(pendingShare)
                    pendingShare = null
                }
                "takeLink" -> {
                    result.success(pendingLink)
                    pendingLink = null
                }
                "shareText" -> result.success(shareText(call.argument<String>("subject"), call.argument<String>("text")))
                "deviceName" -> result.success(deviceName())
                "publicDownloads" -> result.success(publicDownloads())
                else -> result.notImplemented()
            }
        }
        channel = ch
    }

    override fun onResume() {
        super.onResume()
        acquireMulticast()
    }

    override fun onPause() {
        releaseMulticast()
        super.onPause()
    }

    // ------------------------------------------------------------ ядро

    private fun initCore() {
        if (coreReady) return
        try {
            System.loadLibrary("ogneliss_core")
            nativeInit(applicationContext)
            coreReady = true
        } catch (e: Throwable) {
            // Без этого ядро возьмёт запасные DNS-серверы — работать будет.
            Log.w(TAG, "nativeInit: $e")
        }
    }

    private fun acquireMulticast() {
        try {
            if (multicastLock == null) {
                val wifi = applicationContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager ?: return
                multicastLock = wifi.createMulticastLock("ogneliss-share").apply { setReferenceCounted(false) }
            }
            multicastLock?.acquire()
        } catch (e: Exception) {
            Log.w(TAG, "multicast: $e")
        }
    }

    private fun releaseMulticast() {
        try {
            multicastLock?.let { if (it.isHeld) it.release() }
        } catch (e: Exception) {
            Log.w(TAG, "multicast: $e")
        }
    }

    /** Имя, которое человек дал телефону в настройках, иначе — модель. */
    private fun deviceName(): String {
        if (Build.VERSION.SDK_INT >= 25) {
            try {
                val name = Settings.Global.getString(contentResolver, Settings.Global.DEVICE_NAME)
                if (!name.isNullOrBlank()) return name.trim()
            } catch (e: Exception) {
                // нет доступа — возьмём модель
            }
        }
        val maker = (Build.MANUFACTURER ?: "").replaceFirstChar { it.uppercase() }
        val model = Build.MODEL ?: ""
        return if (model.startsWith(maker, ignoreCase = true)) model else "$maker $model".trim()
    }

    @Suppress("DEPRECATION")
    private fun publicDownloads(): String? =
        try {
            Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS)?.absolutePath
        } catch (e: Exception) {
            null
        }

    // ------------------------------------------------------------ ссылки

    /** Открыли ссылку-приглашение: отдать её интерфейсу (сам адрес разберёт ядро). */
    private fun handleLink(intent: Intent?) {
        if (intent?.action != Intent.ACTION_VIEW) return
        val link = intent.dataString ?: return
        // Принято — после поворота экрана та же ссылка повторно не придёт.
        intent.action = Intent.ACTION_MAIN
        pendingLink = link
        channel?.invokeMethod("link", null)
    }

    /** Системное «Поделиться» с текстом: почта, мессенджеры. */
    private fun shareText(subject: String?, text: String?): Boolean {
        if (text.isNullOrEmpty()) return false
        return try {
            val send = Intent(Intent.ACTION_SEND).apply {
                type = "text/plain"
                putExtra(Intent.EXTRA_TEXT, text)
                if (!subject.isNullOrEmpty()) putExtra(Intent.EXTRA_SUBJECT, subject)
            }
            startActivity(Intent.createChooser(send, null))
            true
        } catch (e: Exception) {
            Log.w(TAG, "share text: $e")
            false
        }
    }

    // ------------------------------------------------------------ «Поделиться»

    private fun handleShare(intent: Intent?) {
        val action = intent?.action ?: return
        if (action != Intent.ACTION_SEND && action != Intent.ACTION_SEND_MULTIPLE) return
        val text = intent.getCharSequenceExtra(Intent.EXTRA_TEXT)?.toString()
        val uris = streamsOf(intent, action)
        // Принято — после поворота экрана то же самое повторно не придёт.
        intent.action = Intent.ACTION_MAIN
        if (uris.isEmpty()) {
            if (!text.isNullOrBlank()) deliver(arrayListOf(), text)
            return
        }
        channel?.invokeMethod("sharing", uris.size)
        val app = applicationContext
        Thread {
            val root = File(app.cacheDir, SHARE_DIR)
            cleanupOld(root)
            val files = ArrayList<String>()
            for (u in uris) copySafe(app, u, root)?.let { files.add(it) }
            runOnUiThread { deliver(files, text) }
        }.start()
    }

    private fun deliver(files: ArrayList<String>, text: String?) {
        pendingShare = hashMapOf("files" to files, "text" to text)
        channel?.invokeMethod("shared", null)
    }

    @Suppress("DEPRECATION")
    private fun streamsOf(intent: Intent, action: String): List<Uri> {
        val out = ArrayList<Uri>()
        if (action == Intent.ACTION_SEND) {
            val u: Uri? = if (Build.VERSION.SDK_INT >= 33) {
                intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
            } else {
                intent.getParcelableExtra(Intent.EXTRA_STREAM)
            }
            if (u != null) out.add(u)
        } else {
            val list: ArrayList<Uri>? = if (Build.VERSION.SDK_INT >= 33) {
                intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM, Uri::class.java)
            } else {
                intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM)
            }
            if (list != null) out.addAll(list)
        }
        // Некоторые приложения кладут файлы только в ClipData.
        if (out.isEmpty()) {
            val clip = intent.clipData
            if (clip != null) {
                for (i in 0 until clip.itemCount) clip.getItemAt(i).uri?.let { out.add(it) }
            }
        }
        return out
    }

    companion object {
        private const val TAG = "OgnelissShare"
        private const val CHANNEL = "ogneliss_share/platform"
        private const val SHARE_DIR = "shared_in"
        private const val MAX_AGE_MS = 24L * 60 * 60 * 1000

        @Volatile
        private var coreReady = false

        /**
         * Копия файла под безопасным именем в отдельной папке кэша.
         * Отправитель «Поделиться» сам называет файл, и злонамеренное приложение
         * могло бы подсунуть имя вроде «../shared_prefs/x» или путь к личным
         * файлам этого приложения — такое отклоняется.
         */
        private fun copySafe(ctx: Context, uri: Uri, root: File): String? {
            try {
                when (uri.scheme?.lowercase()) {
                    "content" -> if (uri.authority?.startsWith(ctx.packageName) == true) return null
                    "file" -> {
                        val path = uri.path ?: return null
                        if (isPrivate(ctx, File(path))) return null
                    }
                    else -> return null
                }
                val name = safeName(displayName(ctx, uri))
                val dir = File(root, UUID.randomUUID().toString())
                if (!dir.mkdirs()) return null
                val target = File(dir, name)
                if (!target.canonicalPath.startsWith(dir.canonicalPath + File.separator)) return null
                val input = ctx.contentResolver.openInputStream(uri) ?: return null
                input.use { inp -> FileOutputStream(target).use { out -> inp.copyTo(out, 256 * 1024) } }
                return target.absolutePath
            } catch (e: Exception) {
                Log.w(TAG, "share copy: $e")
                return null
            }
        }

        private fun isPrivate(ctx: Context, f: File): Boolean {
            val roots = listOfNotNull(
                ctx.applicationInfo.dataDir,
                "/data/data/${ctx.packageName}",
                "/data/user/0/${ctx.packageName}",
                ctx.filesDir?.parent,
            ).map { File(it).canonicalPath }
            val p = f.canonicalPath
            return roots.any { p == it || p.startsWith(it + File.separator) }
        }

        private fun displayName(ctx: Context, uri: Uri): String? {
            if (uri.scheme.equals("file", ignoreCase = true)) return uri.lastPathSegment
            return try {
                ctx.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
                    ?.use { c -> if (c.moveToFirst()) c.getString(0) else null }
            } catch (e: Exception) {
                null
            }
        }

        /** Только имя файла: без каталогов, управляющих и запрещённых символов. */
        private fun safeName(raw: String?): String {
            var n = (raw ?: "").substringAfterLast('/').substringAfterLast('\\')
            n = n.replace(Regex("[\\u0000-\\u001f\\u007f:*?\"<>|]"), "_").trim().trim('.')
            if (n.length > 120) {
                val dot = n.lastIndexOf('.')
                val ext = if (dot > 0 && n.length - dot <= 10) n.substring(dot) else ""
                n = n.take(120 - ext.length) + ext
            }
            return if (n.isEmpty()) "file" else n
        }

        private fun cleanupOld(root: File) {
            val now = System.currentTimeMillis()
            root.mkdirs()
            root.listFiles()?.forEach { d ->
                if (now - d.lastModified() > MAX_AGE_MS) d.deleteRecursively()
            }
        }
    }
}
