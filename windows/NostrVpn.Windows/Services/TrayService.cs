using System.Drawing;
using System.Drawing.Drawing2D;
using System.Runtime.InteropServices;
using System.Windows.Forms;
using NostrVpn.Windows.Core;
using NostrVpn.Windows.ViewModels;

namespace NostrVpn.Windows.Services;

public sealed class TrayService : IDisposable
{
    private readonly NotifyIcon _notifyIcon;
    private readonly Icon _normalIcon;
    private readonly Icon _blockedIcon;
    // Built once and shared by every rebuilt menu; ContextMenuStrip.Dispose
    // does not dispose an externally assigned Font.
    private static readonly Font TrayFont = new("Segoe UI", 9f);
    private AppViewModel? _viewModel;
    private Action? _showWindow;
    private Action? _toggleWindow;
    private Action? _quit;

    public TrayService()
    {
        _normalIcon = LoadIcon();
        _blockedIcon = CreateBlockedIcon(_normalIcon);
        _notifyIcon = new NotifyIcon
        {
            Icon = _normalIcon,
            Text = "Nostr VPN",
            Visible = true,
        };
        // Mullvad-style tray behavior: a single left click opens the window
        // (and hides it again when it is already open); the menu handles the
        // rest. DoubleClick is intentionally unused so it doesn't fight the
        // click toggle.
        _notifyIcon.MouseClick += (_, args) =>
        {
            if (args.Button == MouseButtons.Left)
            {
                _toggleWindow?.Invoke();
            }
        };
    }

    public void Attach(AppViewModel viewModel, Action showWindow, Action toggleWindow, Action quit)
    {
        _viewModel = viewModel;
        _showWindow = showWindow;
        _toggleWindow = toggleWindow;
        _quit = quit;
        viewModel.PropertyChanged += (_, _) => Update();
        Update();
    }

    public void Update()
    {
        if (_viewModel is null)
        {
            return;
        }

        _notifyIcon.Text = TruncateTrayText(TrayText(_viewModel));
        _notifyIcon.Icon = _viewModel.State.ExitNodeBlocked ? _blockedIcon : _normalIcon;
        _notifyIcon.ContextMenuStrip?.Dispose();
        _notifyIcon.ContextMenuStrip = BuildMenu(_viewModel);
    }

    public void Dispose()
    {
        _notifyIcon.Visible = false;
        _notifyIcon.Dispose();
        _normalIcon.Dispose();
        _blockedIcon.Dispose();
    }

    private ContextMenuStrip BuildMenu(AppViewModel viewModel)
    {
        // Minimal Mullvad-style tray menu: VPN toggle + status, the internet
        // source picker, then open/quit. Device management lives in the app
        // window (Home / Account / Internet). The font is pinned because the
        // WinForms menu font auto-scales on top of the WPF process DPI
        // awareness and renders oversized at >100% display scaling.
        var menu = new ContextMenuStrip { Font = TrayFont };

        var vpnToggle = Item("Nostr VPN", async (_, _) => await viewModel.ToggleVpnAsync(),
            viewModel.State.VpnControlSupported && !viewModel.ActionInFlight);
        vpnToggle.Checked = viewModel.State.VpnEnabled;
        menu.Items.Add(vpnToggle);
        // Status subtitle right below the toggle, mirrors the macOS row's
        // second line and reuses the same VpnStatusText that the footer shows.
        menu.Items.Add(Item(viewModel.VpnStatusText, (_, _) => { }, false));

        menu.Items.Add(new ToolStripSeparator());

        var network = viewModel.ActiveNetwork;
        if (network is not null)
        {
            var exitNodes = new ToolStripMenuItem("Internet Source");
            if (!string.IsNullOrWhiteSpace(viewModel.State.ExitNodeStatusText))
            {
                exitNodes.DropDownItems.Add(Item(viewModel.State.ExitNodeStatusText, (_, _) => { }, false));
            }
            var offerExit = Item("Share This Device",
                async (_, _) => await viewModel.SetAdvertiseExitNodeAsync(!viewModel.State.AdvertiseExitNode));
            offerExit.Checked = viewModel.State.AdvertiseExitNode;
            exitNodes.DropDownItems.Add(offerExit);
            exitNodes.DropDownItems.Add(new ToolStripSeparator());
            var noExit = Item("This device", async (_, _) => await viewModel.SelectDirectExitAsync());
            noExit.Checked = viewModel.State.InternetSource == "direct";
            exitNodes.DropDownItems.Add(noExit);
            if (viewModel.PaidRouteMarketVisible)
            {
                var paidAutomatic = Item("Paid · Automatic · Experimental", async (_, _) => await viewModel.SelectPaidAutomaticExitAsync());
                paidAutomatic.Checked = viewModel.State.InternetSource == "paid_automatic";
                exitNodes.DropDownItems.Add(paidAutomatic);
                var paidManual = Item("Paid · Choose manually", async (_, _) => await viewModel.SelectPaidManualExitAsync());
                paidManual.Checked = viewModel.State.InternetSource == "paid_manual";
                exitNodes.DropDownItems.Add(paidManual);
            }
            var wireGuard = Item("WireGuard VPN", async (_, _) => await viewModel.SelectWireGuardUpstreamExitAsync(), viewModel.State.WireguardExitConfigured);
            wireGuard.Checked = viewModel.State.InternetSource == "wireguard";
            exitNodes.DropDownItems.Add(wireGuard);
            foreach (var participant in network.Participants.Where(participant => participant.OffersExitNode && !participant.IsSelf))
            {
                var item = Item(DeviceName(participant), async (_, _) => await viewModel.SelectPeerExitAsync(participant.Npub));
                item.Checked = viewModel.State.InternetSource == "private_vpn" && viewModel.State.ExitNode == participant.Npub;
                exitNodes.DropDownItems.Add(item);
            }
            menu.Items.Add(exitNodes);
        }

        menu.Items.Add(new ToolStripSeparator());
        menu.Items.Add(Item("Open Nostr VPN", (_, _) => _showWindow?.Invoke()));
        menu.Items.Add(Item("Quit", (_, _) => _quit?.Invoke()));
        return menu;
    }

    private static ToolStripMenuItem Item(string text, EventHandler onClick, bool enabled = true)
    {
        var item = new ToolStripMenuItem(text) { Enabled = enabled };
        item.Click += onClick;
        return item;
    }

    private static Icon LoadIcon()
    {
        foreach (var filename in new[] { "nostr-vpn-tray.ico", "nostr-vpn.ico" })
        {
            var iconPath = Path.Combine(AppContext.BaseDirectory, "Assets", filename);
            if (File.Exists(iconPath))
            {
                return new Icon(iconPath);
            }
        }

        return (Icon)SystemIcons.Application.Clone();
    }

    private static Icon CreateBlockedIcon(Icon baseIcon)
    {
        using var bitmap = baseIcon.ToBitmap();
        using var graphics = Graphics.FromImage(bitmap);
        graphics.SmoothingMode = SmoothingMode.AntiAlias;
        var diameter = Math.Max(6, Math.Min(bitmap.Width, bitmap.Height) / 3);
        var x = bitmap.Width - diameter - 1;
        var y = 1;
        using var brush = new SolidBrush(Color.FromArgb(220, 38, 38));
        graphics.FillEllipse(brush, x, y, diameter, diameter);
        var handle = bitmap.GetHicon();
        try
        {
            return (Icon)Icon.FromHandle(handle).Clone();
        }
        finally
        {
            DestroyIcon(handle);
        }
    }

    private static string TrayText(AppViewModel viewModel)
    {
        var status = !string.IsNullOrWhiteSpace(viewModel.State.ExitNodeStatusText)
            ? viewModel.State.ExitNodeStatusText
            : viewModel.State.VpnStatus;
        return $"Nostr VPN - {status}";
    }

    private static string TruncateTrayText(string value)
    {
        return value.Length <= 63 ? value : value[..60] + "...";
    }

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool DestroyIcon(IntPtr handle);

    private static string DeviceName(NativeParticipantState participant)
    {
        if (!string.IsNullOrWhiteSpace(participant.MagicDnsName))
        {
            return participant.MagicDnsName;
        }
        if (!string.IsNullOrWhiteSpace(participant.Alias))
        {
            return participant.Alias;
        }
        return participant.Npub.Length > 16
            ? $"{participant.Npub[..10]}...{participant.Npub[^6..]}"
            : participant.Npub;
    }
}
