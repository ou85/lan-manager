export function ipNumber(ip) {
  const p=ip.split('.');
  if(p.length!==4||p.some(x=>!/^\d{1,3}$/.test(x)||Number(x)>255))return null;
  return p.reduce((n,x)=>n*256+Number(x),0);
}
export function ipString(n){return [24,16,8,0].map(shift=>(n>>>shift)&255).join('.');}
export function subnetInfo(cidr){
  const [ip,p,...rest]=cidr.split('/');const n=ipNumber(ip);
  if(n===null||p===undefined||!/^\d{1,2}$/.test(p)||Number(p)>32||rest.length)return null;
  const prefix=Number(p),size=2**(32-prefix),start=Math.floor(n/size)*size;
  return {start,end:start+size-1,prefix,first:prefix>=31?start:start+1,last:prefix>=31?start+size-1:start+size-2,total:prefix>=31?size:size-2};
}
export function allocation(subnet,devices){
  const net=subnetInfo(subnet.cidr);if(!net)return null;
  const assigned=new Set(devices.map(d=>ipNumber(d.ip)).filter(n=>n!==null&&n>=net.first&&n<=net.last));
  const gateway=ipNumber(subnet.gateway);
  if(gateway!==null&&gateway>=net.first&&gateway<=net.last)assigned.add(gateway);
  let next=net.first;while(next<=net.last&&assigned.has(next))next++;
  return {...net,used:assigned.size,free:net.total-assigned.size,next:next<=net.last?ipString(next):null,assigned};
}
