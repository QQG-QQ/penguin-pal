export type VisualKind='none'|'sticker'|'mood_card'|'scene'
export interface VisualSettings {enabled:boolean;automatic:boolean;managedRuntime:boolean;baseUrl:string;workflow:string;character:string;cooldownMinutes:number;dailyLimit:number;timeoutSeconds:number;wechatEnabled:boolean;hasWorkflow?:boolean;hasReference?:boolean}
export interface VisualEntry {id:string;kind:VisualKind;caption:string;emotion:string;personality:string;mood:number;at:number;messageId:string;path:string;reused:boolean}
export interface VisualBoard {settings:VisualSettings;entries:VisualEntry[];error:string;job:{state?:string;kind?:VisualKind;message?:string};runtime?:{installed:boolean;state:string;root?:string;model?:string;baseUrl:string;idleSeconds:number;error?:string;install?:{state?:string;message?:string;progress?:number}};wechat:{connected:boolean;account?:string;error?:string;queued?:number;processing?:number;sending?:number;failed?:number;uncertain?:number;imagesUncertain?:number;receive?:{polledAt?:number;received?:number;accepted?:number;ignored?:number}}}
export const visualLabel=(kind:VisualKind)=>({none:'图片',sticker:'表情包',mood_card:'今日心情卡',scene:'小剧场'}[kind])
export const defaultVisualSettings=():VisualSettings=>({enabled:false,automatic:true,managedRuntime:false,baseUrl:'http://127.0.0.1:8188',workflow:'',character:'expressive everyday chat reaction meme, freely choose a funny animal, cartoon face or simple everyday subject to match the conversation',cooldownMinutes:30,dailyLimit:4,timeoutSeconds:180,wechatEnabled:false})

// Ordinary chat memes use text-to-image; reference workflows remain optional.
export function managedWorkflow(model='DreamShaper8_LCM.safetensors'){
  const graph=JSON.parse(starterWorkflow(model))
  Object.assign(graph['5'].inputs,{steps:6,cfg:1.5,sampler_name:'lcm',scheduler:'normal',denoise:1})
  return JSON.stringify(graph,null,2)
}

// Optional starter for an installed SD 1.x checkpoint. Custom API workflows remain supported.
export function starterWorkflow(model:string){
  if(!model.trim())throw Error('请先选择已安装的 SD 1.x 模型')
  return JSON.stringify({
    '1':{class_type:'CheckpointLoaderSimple',inputs:{ckpt_name:model}},
    '2':{class_type:'CLIPTextEncode',inputs:{text:'{{prompt}}',clip:['1',1]}},
    '3':{class_type:'CLIPTextEncode',inputs:{text:'text, watermark, logo, blurry, deformed, extra limbs',clip:['1',1]}},
    '4':{class_type:'EmptyLatentImage',inputs:{width:512,height:512,batch_size:1}},
    '5':{class_type:'KSampler',inputs:{seed:'{{seed}}',steps:20,cfg:7,sampler_name:'euler',scheduler:'normal',denoise:1,model:['1',0],positive:['2',0],negative:['3',0],latent_image:['4',0]}},
    '6':{class_type:'VAEDecode',inputs:{samples:['5',0],vae:['1',2]}},
    '7':{class_type:'SaveImage',inputs:{filename_prefix:'PenguinPal',images:['6',0]}},
  },null,2)
}
