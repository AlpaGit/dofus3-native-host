using System;
using System.Runtime.InteropServices;
using UnityEngine;

namespace DofusNativeHarness
{
    [DefaultExecutionOrder(-32000)]
    internal sealed class NativeHostDriver : MonoBehaviour
    {
        private static bool s_created;
        private bool _initialized;

        [DllImport("DofusNativeBootstrap", CallingConvention = CallingConvention.Winapi)]
        private static extern int DNB_NotifyUnityReady();

        [DllImport("DofusNativeBootstrap", CallingConvention = CallingConvention.Winapi)]
        private static extern void DNB_Tick();

        [DllImport("DofusNativeBootstrap", CallingConvention = CallingConvention.Winapi)]
        private static extern void DNB_Shutdown();

        [RuntimeInitializeOnLoadMethod(RuntimeInitializeLoadType.BeforeSceneLoad)]
        private static void Install()
        {
            if (s_created)
                return;

            s_created = true;
            var host = new GameObject("[DofusNativeHost]");
            DontDestroyOnLoad(host);
            host.AddComponent<NativeHostDriver>();
        }

        private void Awake()
        {
            try
            {
                int result = DNB_NotifyUnityReady();
                _initialized = result == 0 || result == 1;
                if (!_initialized)
                    Debug.LogError($"Dofus Native Bootstrap failed with state {result}.");
            }
            catch (Exception exception)
            {
                Debug.LogException(exception);
            }
        }

        private void Update()
        {
            if (_initialized)
                DNB_Tick();
        }

        private void OnApplicationQuit()
        {
            if (!_initialized)
                return;

            DNB_Shutdown();
            _initialized = false;
        }
    }
}
