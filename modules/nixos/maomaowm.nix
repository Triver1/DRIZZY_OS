{inputs, config, lib, pkgs, ... }:

{
  imports = [
     inputs.mango.nixosModules.mango
  ];
  programs.mango.enable = true;

  home-manager.sharedModules = [
    inputs.mango.hmModules.mango
    {
      wayland.windowManager.mango = {
        enable = true;
        settings = ''
# Window effect (macOS frosted glass style)
blur=1
blur_layer=1
blur_optimized=1
blur_params_num_passes = 3
blur_params_radius = 8
blur_params_noise = 0.01
blur_params_brightness = 1.0
blur_params_contrast = 1.0
blur_params_saturation = 1.3

# Depth and Shadows (Crucial for macOS look)
shadows = 1
layer_shadows = 1
shadow_only_floating = 0
shadows_size = 20
shadows_blur = 25
shadowscolor= 0x00000055

# Rounded Corners & Opacity
border_radius=12
no_radius_when_single=0
focused_opacity=0.97
unfocused_opacity=0.9

# Animation Configuration (macOS zoom effect)
animations=1
layer_animations=1
animation_type_open=zoom
animation_type_close=zoom
animation_fade_in=1
animation_fade_out=1
tag_animation_direction=1
zoom_initial_ratio=0.85
zoom_end_ratio=0.85
fadein_begin_opacity=0.5
fadeout_begin_opacity=0.8
animation_duration_move=250
animation_duration_open=250
animation_duration_tag=200
animation_duration_close=250
animation_curve_open=0.25,1.0,0.25,1.0
animation_curve_move=0.25,1.0,0.25,1.0
animation_curve_tag=0.25,1.0,0.25,1.0
animation_curve_close=0.25,1.0,0.25,1.0

# Scroller Layout Setting
scroller_structs=20
scroller_default_proportion=0.5
scroller_focus_center=1
scroller_prefer_center=0
edge_scroller_pointer_focus=0
scroller_default_proportion_single=1.0
scroller_proportion_preset=0.5,0.8,1.0

# Master-Stack Layout Setting
new_is_master=0
default_mfact=0.6
default_nmaster=1
smartgaps=1

# Overview Setting
hotarea_size=10
enable_hotarea=1
ov_tab_mode=0
overviewgappi=10
overviewgappo=40

# Misc
no_border_when_single=0
axis_bind_apply_timeout=100
focus_on_activate=1
inhibit_regardless_of_visibility=0
sloppyfocus=1
warpcursor=1
focus_cross_monitor=0
focus_cross_tag=0
enable_floating_snap=0
snap_distance=30
cursor_size=24
drag_tile_to_tile=1

# keyboard
repeat_rate=25
repeat_delay=600
numlockon=0
xkb_rules_layout=us
xkb_rules_options=caps:escape

# Trackpad
disable_trackpad=0
tap_to_click=1
tap_and_drag=1
drag_lock=1
trackpad_natural_scrolling=1
disable_while_typing=1
left_handed=0
middle_button_emulation=0
swipe_min_threshold=1

# mouse
mouse_natural_scrolling=0

# Appearance (Clean margins and macOS Palette)
gappih=8
gappiv=8
gappoh=16
gappov=16
scratchpad_width_ratio=0.8
scratchpad_height_ratio=0.9
borderpx=1
rootcolor=0x1e1e1eff
bordercolor=0x333333ff
focuscolor=0x007affff 
maximizescreencolor=0x34c759ff
urgentcolor=0xff3b30ff
scratchpadcolor=0x5856d6ff
globalcolor=0xaf52deff
overlaycolor=0x00000000

# layout support:
tagrule=id:1,layout_name:scroller
tagrule=id:2,layout_name:tile
tagrule=id:3,layout_name:tile
tagrule=id:4,layout_name:tile
tagrule=id:5,layout_name:tile
tagrule=id:6,layout_name:tile
tagrule=id:7,layout_name:tile
tagrule=id:8,layout_name:tile
tagrule=id:9,layout_name:tile

# Key Bindings
bind=SUPER,s,setlayout,scroller
bind=SUPER,t,setlayout,tile

# hardware keys: brightness and volume
bind=NONE,XF86MonBrightnessUp,spawn,brightnessctl --class=backlight set +10%
bind=NONE,XF86MonBrightnessDown,spawn,brightnessctl --class=backlight set 10%-
bind=NONE,XF86AudioRaiseVolume,spawn,wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%+
bind=NONE,XF86AudioLowerVolume,spawn,wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%-
bind=NONE,XF86AudioMute,spawn,wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle
bind=NONE,XF86AudioMicMute,spawn,wpctl set-mute @DEFAULT_AUDIO_SOURCE@ toggle
bind=NONE,XF86AudioPlay,spawn,playerctl play-pause
bind=NONE,XF86AudioNext,spawn,playerctl next
bind=NONE,XF86AudioPrev,spawn,playerctl previous
bind=NONE,XF86AudioStop,spawn,playerctl stop

# reload config
bind=SUPER,r,reload_config

# menu and terminal
bind=SUPER,space,spawn,rofi -show drun
bind=SUPER,Return,spawn,kitty
bind=SUPER,b,spawn,zen-beta
bind=SUPER,e,spawn,kitty -e yazi

# exit
bind=SUPER,m,quit
bind=SUPER,q,killclient,

# switch window focus
bind=SUPER+SHIFT,k,tagtoleft
bind=SUPER+SHIFT,j,tagtoright
bind=SUPER,k,focusdir,up
bind=SUPER,j,focusdir,down
bind=SUPER,h,focusdir,left
bind=SUPER,l,focusdir,right

# WINDOW TAG MANAGEMENT
bind=SUPER+SHIFT,1,toggletag,1
bind=SUPER+SHIFT,2,toggletag,2
bind=SUPER+SHIFT,3,toggletag,3
bind=SUPER+SHIFT,4,toggletag,4
bind=SUPER+SHIFT,5,toggletag,5
bind=SUPER+SHIFT,6,toggletag,6
bind=SUPER+SHIFT,7,toggletag,7
bind=SUPER+SHIFT,8,toggletag,8
bind=SUPER+SHIFT,9,toggletag,9

# swap window
bind=SUPER+CTRL,k,exchange_client,up
bind=SUPER+CTRL,j,exchange_client,down
bind=SUPER+CTRL,h,exchange_client,left
bind=SUPER+CTRL,l,exchange_client,right

# switch window status
bind=SUPER,g,toggleglobal,
bind=SUPER,o,toggleoverview,
bind=SUPER,v,togglefloating,
bind=SUPER,a,togglemaximizescreen,
bind=SUPER,f,togglefullscreen,
bind=SUPER+SHIFT,f,togglefakefullscreen,
bind=SUPER,i,minimized,
bind=SUPER+SHIFT,I,restore_minimized
bind=SUPER,z,toggle_scratchpad

# scroller layout
bind=SUPER,e,set_proportion,1.0
bind=SUPER,x,switch_proportion_preset,

# switch layout
bind=SUPER,n,focusstack, next
bind=SUPER,w,switch_layout

# tag switch
bind=SUPER,1,view,1,0
bind=SUPER,2,view,2,0
bind=SUPER,3,view,3,0
bind=SUPER,4,view,4,0
bind=SUPER,5,view,5,0
bind=SUPER,6,view,6,0
bind=SUPER,7,view,7,0
bind=SUPER,8,view,8,0
bind=SUPER,9,view,9,0

# MONITOR RULES
monitorrule=name:eDP-1,width:3200,height:2000,refresh:120,x:0,y:0,scale:1.75,vrr:0,rr:0
monitorrule=name:DP-3,width:3440,height:1440,refresh:180,x:0,y:0,scale:1,vrr:0,rr:0

# monitor switch
bind=SUPER+SHIFT,h,focusmon,left
bind=SUPER+SHIFT,l,focusmon,right
bind=SUPER+CTRL+SHIFT,h,tagmon,left
bind=SUPER+CTRL+SHIFT,l,tagmon,right

# gaps
bind=SUPER+SHIFT,equal,incgaps,1
bind=SUPER+SHIFT,minus,incgaps,-1
bind=SUPER+SHIFT,g,togglegaps

# movewin
bind=SUPER+ALT,k,movewin,+0,-50
bind=SUPER+ALT,j,movewin,+0,+50
bind=SUPER+ALT,h,movewin,-50,+0
bind=SUPER+ALT,l,movewin,+50,+0

# resizewin
bind=SUPER,equal,resizewin,+50,+0
bind=SUPER,minus,resizewin,-50,+0

# Mouse Button Bindings
mousebind=SUPER,btn_left,moveresize,curmove
mousebind=NONE,btn_middle,togglemaximizescreen,0
mousebind=SUPER,btn_right,moveresize,curresize
mousebind=NONE,btn_left,toggleoverview,-1
mousebind=NONE,btn_right,killclient,0

# Axis Bindings
axisbind=SUPER,UP,viewtoleft_have_client
axisbind=SUPER,DOWN,viewtoright_have_client

# layer rule
layerrule=animation_type_open:zoom,layer_name:rofi
layerrule=animation_type_close:zoom,layer_name:rofi
        '';
        autostart_sh = ''
        noctalia-shell
        '';
      };
    }
  ];
}
