package com.trykacz.nectan

import android.graphics.Color
import android.os.Bundle
import io.flutter.embedding.android.FlutterFragmentActivity

class MainActivity : FlutterFragmentActivity() {
    init {
        System.loadLibrary("rust_lib_nectan_mobile")
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.navigationBarColor = Color.parseColor("#1a1a1a")
    }
}