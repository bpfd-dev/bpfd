# Deploying the bpfman-operator

The `bpfman-operator` repository exists in order to deploy and manage bpfman within a Kubernetes cluster.
This operator was built utilizing some great tooling provided by the
[operator-sdk library](https://sdk.operatorframework.io/).
A great first step in understanding some of the functionality can be to just run `make help`.

## Deploy bpfman Operation

The `bpfman-operator` is running as a Deployment with a ReplicaSet of one.
It runs on the control plane and is composed of the containers `bpfman-operator` and
`kube-rbac-proxy`.
The operator is responsible for launching the bpfman Daemonset, which runs on every node.
The bpfman Daemonset is composed of the containers `bpfman`, `bpfman-agent`, and `node-driver-registrar`.

Described below are two ways to deploy bpfman in a Kubernetes cluster:

* [Deploy Locally via KIND](#deploy-locally-via-kind): Easiest way to deploy bpfman in a Kubernetes cluster
  and great for testing.
* [Deploy To Openshift Cluster](#deploy-to-openshift-cluster): Special steps are needed to deploy on an
  Openshift cluster because SELinux is enable.

### Deploy Locally via KIND

After reviewing the possible make targets it's quick and easy to get bpfman deployed locally on your system
via a [KIND cluster](https://kind.sigs.k8s.io/) with:

```bash
cd bpfman-operator
make run-on-kind
```

!!! Note
    By default, bpfman-operator deploys bpfman with CSI enabled.
    CSI requires Kubernetes v1.26 due to a PR
    ([kubernetes/kubernetes#112597](https://github.com/kubernetes/kubernetes/pull/112597))
    that addresses a gRPC Protocol Error that was seen in the CSI client code and it doesn't
    appear to have been backported.
    It is recommended to install kind v0.20.0 or later.

### Deploy To Openshift Cluster

The recommend way of deploying bpfman to an OpenShift cluster is via the
OpenShift Console and using Operator Hub.
This is described in
[OperatorHub via OpenShift Console](../developer-guide/develop-operator.md#operatorhub-via-openshift-console).
For other options, see
[Deploy To Existing Cluster](../developer-guide/develop-operator.md#deploy-to-existing-cluster).

## API Types Overview

Refer to  [api-spec.md](../developer-guide/api-spec.md) for a more detailed description of all the bpfman Kubernetes API types.

### Cluster Scoped Versus Namespaced Scoped CRDs

For security reasons, cluster admins may want to limit certain applications to only loading eBPF programs
within a given namespace.
To provide these tighter controls on eBPF program loading, some of the bpfman Custom Resource Definitions (CRDs)
are Namespace scoped.
Not all eBPF programs make sense to be namespaced scoped.
The namespaced scoped CRDs use the "<ProgramType\>NsProgram" identifier and cluster scoped CRDs to use "<ProgramType\>Program"
identifier.

### Multiple Program CRDs

The multiple `*Program` CRDs are the bpfman Kubernetes API objects most relevant to users and can be used to
understand clusterwide state for an eBPF program.
It's designed to express how, and where eBPF programs are to be deployed within a Kubernetes cluster.
Currently bpfman supports:

* `fentryProgram`
* `fexitProgram`
* `kprobeProgram`
* `tcProgram` and `tcNsProgram`
* `tcxProgram` and `tcxNsProgram`
* `tracepointProgram`
* `uprobeProgram` and `uprobeNsProgam`
* `xdpProgram` and `xdpNsProgram`

There is also the `bpfApplication` and `bpfNsApplication` CRDs, which are
designed for managing eBPF programs at an application level within a Kubernetes cluster.
These CRD allows Kubernetes users to define which eBPF programs are essential for an application's operations
and specify how these programs should be deployed across the cluster.
With cluster scoped variant (`bpfApplication`), any variation of the cluster scoped
eBPF programs can be loaded.
With namespace scoped variant (`bpfNsApplication`), any variation of the namespace scoped
eBPF programs can be loaded.

### BpfProgram and BpfNsProgram CRD

The `BpfProgram` and  `BpfNsProgram` CRDs are used internally by the bpfman-deployment to keep track of per
node bpfman state such as map pin points, and to report node specific errors back to the user.
Kubernetes users/controllers are only allowed to view these objects, NOT create or edit them.

Applications wishing to use bpfman to deploy/manage their eBPF programs in Kubernetes will make use of this
object to find references to the bpfMap pin points (`spec.maps`) in order to configure their eBPF programs.

## Deploy an eBPF Program to the cluster

There are sample yamls for each of the support program type in the
[bpfman-operator/config/samples](https://github.com/bpfman/bpfman-operator/tree/main/config/samples)
directory.

### Deploy Cluster Scoped Sample

Any of the cluster scoped samples can be applied as is.
To test the deployment simply deploy one of the sample `xdpPrograms`:

```bash
cd bpfman-operator/
kubectl apply -f config/samples/bpfman.io_v1alpha1_xdp_pass_xdpprogram.yaml
```

If loading of the XDP Program to the selected nodes was successful it will be reported
back to the user via the `xdpProgram`'s status field:

```bash
$ kubectl get xdpprogram xdp-pass-all-nodes -o yaml
apiVersion: bpfman.io/v1alpha1
kind: XdpProgram
metadata:
  annotations:
    kubectl.kubernetes.io/last-applied-configuration: |
      {"apiVersion":"bpfman.io/v1alpha1","kind":"XdpProgram","metadata":{"annotations":{},"labels":{"app.kubernetes.io/name":"xdpprogram"},"name":"xdp-pass-all-nodes"},"spec":{"bpffunctionname":"pass","bytecode":{"image":{"url":"quay.io/bpfman-bytecode/xdp_pass:latest"}},"globaldata":{"GLOBAL_u32":[13,12,11,10],"GLOBAL_u8":[1]},"interfaceselector":{"primarynodeinterface":true},"nodeselector":{},"priority":0}}
  creationTimestamp: "2023-11-07T19:16:39Z"
  finalizers:
  - bpfman.io.operator/finalizer
  generation: 2
  labels:
    app.kubernetes.io/name: xdpprogram
  name: xdp-pass-all-nodes
  resourceVersion: "157187"
  uid: 21c71a61-4e73-44eb-9b49-07af2866d25b
spec:
  bpffunctionname: pass
  bytecode:
    image:
      imagepullpolicy: IfNotPresent
      url: quay.io/bpfman-bytecode/xdp_pass:latest
  globaldata:
    GLOBAL_u8: AQ==
    GLOBAL_u32: DQwLCg==
  interfaceselector:
    primarynodeinterface: true
  mapownerselector: {}
  nodeselector: {}
  priority: 0
  proceedon:
  - pass
  - dispatcher_return
status:
  conditions:
  - lastTransitionTime: "2023-11-07T19:16:42Z"
    message: bpfProgramReconciliation Succeeded on all nodes
    reason: ReconcileSuccess
    status: "True"
    type: ReconcileSuccess
```

To see information in listing form simply run:

```bash
$ kubectl get xdpprogram -o wide
NAME                 BPFFUNCTIONNAME   NODESELECTOR   PRIORITY   INTERFACESELECTOR               PROCEEDON
xdp-pass-all-nodes   pass              {}             0          {"primarynodeinterface":true}   ["pass","dispatcher_return"]
```

To view each attachment point on each node, use the `bpfProgram` object:

```bash
$ kubectl get bpfprograms
NAME                          TYPE   STATUS         AGE
xdp-pass-all-nodes-f3def00d   xdp    bpfmanLoaded   56s


$ kubectl get bpfprograms xdp-pass-all-nodes-f3def00d -o yaml
apiVersion: bpfman.io/v1alpha1
kind: BpfProgram
metadata:
  annotations:
    bpfman.io.xdpprogramcontroller/interface: eth0
    bpfman.io/ProgramId: "26577"
    bpfman.io/bpfProgramAttachPoint: eth0
  creationTimestamp: "2024-12-18T22:26:55Z"
  finalizers:
  - bpfman.io.xdpprogramcontroller/finalizer
  generation: 1
  labels:
    bpfman.io/appProgramId: ""
    bpfman.io/ownedByProgram: xdp-pass-all-nodes
    kubernetes.io/hostname: bpfman-deployment-control-plane
  name: xdp-pass-all-nodes-f3def00d
  ownerReferences:
  - apiVersion: bpfman.io/v1alpha1
    blockOwnerDeletion: true
    controller: true
    kind: XdpProgram
    name: xdp-pass-all-nodes
    uid: 7685a5b6-a626-4483-8c20-06b29643a2a8
  resourceVersion: "8430"
  uid: 83c5a80d-2dca-46ce-806b-6fdf7bde901f
spec:
  type: xdp
status:
  conditions:
  - lastTransitionTime: "2024-12-18T22:27:11Z"
    message: Successfully loaded bpfProgram
    reason: bpfmanLoaded
    status: "True"
    type: Loaded
```

### Deploy Namespace Scoped Sample

The namespace scoped samples need a namespace and pods to attach to.
A yaml has been created that will create a `Namespace` called "acme".
([bpfman-operator/hack/namespace_scoped.yaml](https://github.com/bpfman/bpfman-operator/blob/main/hack/namespace_scoped.yaml)).
The reason for namespace scoped CRDs is to limit an application or user to a namespace.
To this end, this yaml also creates a `ServiceAccount`, `Role`, `RoleBinding` and `Secret`.

```bash
cd bpfman-operator
kubectl apply -f hack/namespace_scoped.yaml 
  namespace/acme created
  serviceaccount/test-account created
  role.rbac.authorization.k8s.io/test-account created
  rolebinding.rbac.authorization.k8s.io/test-account created
  secret/test-account-token created
```

To create a `kubeconfig` file that limits access to the created namespace, use the script 
[bpfman-operator/hack/namespace_scoped.sh](https://github.com/bpfman/bpfman-operator/blob/main/hack/nginx-deployment.sh).
The script needs to know the name of the `Cluster`, `Namespace`, `Service Account` and `Secret`.
The script defaults these fields to what is currently in 
[bpfman-operator/hack/namespace_scoped.yaml](https://github.com/bpfman/bpfman-operator/blob/main/hack/namespace_scoped.yaml).
However, if a file is passed to the script, it will look for the `Secret` object and attempt to extract the values. 
This can be used if the names are changed or a different yaml file is used.
The output of the script is the contents of a `kubeconfig`.
This can be printed to the console or redirected to a file.

```bash
./hack/namespace_scoped.sh hack/namespace_scoped.yaml > /tmp/kubeconfig 
```

To use the `kubeconfig` file, select the session to limit access in and run:

```bash
export KUBECONFIG=/tmp/kubeconfig
```

From within this limited access session, a sample `nginx` deployment can be created in the same namespace using
[bpfman-operator/hack/namespace_scoped.yaml](https://github.com/bpfman/bpfman-operator/blob/main/hack/nginx-deployment.yaml).

```bash
kubectl apply -f hack/nginx-deployment.yaml
  deployment.apps/nginx-deployment created
```

Finally, load any of the namespaced samples from
[bpfman-operator/config/samples](https://github.com/bpfman/bpfman-operator/tree/main/config/samples).
They are of the format: `bpfman.io_v1alpha1_*nsprogram.yaml`

```bash
kubectl apply -f config/samples/bpfman.io_v1alpha1_tc_pass_tcnsprogram.yaml 
  tcnsprogram.bpfman.io/tc-containers created
```

The status for each namespaced program is reported via the \*NsProgram status field and further
information can be seen in the resulting BpfNsProgram CRDs.
As an example, the following commands display the information of the TC program loaded in the acme
namespace with the command above.

```bash
$ kubectl get tcnsprograms
NAME            BPFFUNCTIONNAME   NODESELECTOR   STATUS
tc-containers   pass              {}             ReconcileSuccess


$ kubectl get tcnsprograms tc-containers -o yaml
apiVersion: bpfman.io/v1alpha1
kind: TcNsProgram
metadata:
  annotations:
    kubectl.kubernetes.io/last-applied-configuration: |
      {"apiVersion":"bpfman.io/v1alpha1","kind":"TcNsProgram","metadata":{"annotations":{},"labels":{"app.kubernetes.io/name":"tcnsprogram"},"name":"tc-containers","namespace":"acme"},"spec":{"bpffunctionname":"pass","bytecode":{"image":{"url":"quay.io/bpfman-bytecode/tc_pass:latest"}},"containers":{"containernames":["nginx"],"pods":{"matchLabels":{"app":"nginx"}}},"direction":"ingress","globaldata":{"GLOBAL_u32":[13,12,11,10],"GLOBAL_u8":[1]},"interfaceselector":{"interfaces":["eth0"]},"nodeselector":{},"priority":0}}
  creationTimestamp: "2024-12-18T22:22:52Z"
  finalizers:
  - bpfman.io.operator/finalizer
  generation: 2
  labels:
    app.kubernetes.io/name: tcnsprogram
  name: tc-containers
  namespace: acme
  resourceVersion: "7993"
  uid: 49291f28-49dc-4486-9119-af7c31569de3
spec:
  bpffunctionname: pass
  bytecode:
    image:
      imagepullpolicy: IfNotPresent
      url: quay.io/bpfman-bytecode/tc_pass:latest
  containers:
    containernames:
    - nginx
    pods:
      matchLabels:
        app: nginx
  direction: ingress
  globaldata:
    GLOBAL_u8: AQ==
    GLOBAL_u32: DQwLCg==
  interfaceselector:
    interfaces:
    - eth0
  mapownerselector: {}
  nodeselector: {}
  priority: 0
  proceedon:
  - pipe
  - dispatcher_return
status:
  conditions:
  - lastTransitionTime: "2024-12-18T22:23:11Z"
    message: bpfProgramReconciliation Succeeded on all nodes
    reason: ReconcileSuccess
    status: "True"
    type: ReconcileSuccess
```

To view each attachment point on each node, use the `bpfNsProgram` object:

```bash
$ kubectl get bpfnsprograms
NAME                     TYPE   STATUS         AGE
tc-containers-6494dbed   tc     bpfmanLoaded   12m
tc-containers-7dcde5ab   tc     bpfmanLoaded   12m


$ kubectl get bpfnsprograms tc-containers-6494dbed -o yaml
apiVersion: bpfman.io/v1alpha1
kind: BpfNsProgram
metadata:
  annotations:
    bpfman.io.tcnsprogramcontroller/containerpid: "3256"
    bpfman.io.tcnsprogramcontroller/interface: eth0
    bpfman.io/ProgramId: "26575"
    bpfman.io/bpfProgramAttachPoint: eth0-ingress-nginx-deployment-57d84f57dc-lgc6f-nginx
  creationTimestamp: "2024-12-18T22:23:08Z"
  finalizers:
  - bpfman.io.tcnsprogramcontroller/finalizer
  generation: 1
  labels:
    bpfman.io/appProgramId: ""
    bpfman.io/ownedByProgram: tc-containers
    kubernetes.io/hostname: bpfman-deployment-control-plane
  name: tc-containers-6494dbed
  namespace: acme
  ownerReferences:
  - apiVersion: bpfman.io/v1alpha1
    blockOwnerDeletion: true
    controller: true
    kind: TcNsProgram
    name: tc-containers
    uid: 49291f28-49dc-4486-9119-af7c31569de3
  resourceVersion: "7992"
  uid: c913eea4-71e0-4d5d-b664-078abac36c40
spec:
  type: tc
status:
  conditions:
  - lastTransitionTime: "2024-12-18T22:23:11Z"
    message: Successfully loaded bpfProgram
    reason: bpfmanLoaded
    status: "True"
    type: Loaded
```
