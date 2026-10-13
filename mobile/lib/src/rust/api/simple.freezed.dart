// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'simple.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$AddRemoteDeviceEvent {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is AddRemoteDeviceEvent);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'AddRemoteDeviceEvent()';
}


}

/// @nodoc
class $AddRemoteDeviceEventCopyWith<$Res>  {
$AddRemoteDeviceEventCopyWith(AddRemoteDeviceEvent _, $Res Function(AddRemoteDeviceEvent) __);
}


/// Adds pattern-matching-related methods to [AddRemoteDeviceEvent].
extension AddRemoteDeviceEventPatterns on AddRemoteDeviceEvent {
/// A variant of `map` that fallback to returning `orElse`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( AddRemoteDeviceEvent_WaitingForResponse value)?  waitingForResponse,TResult Function( AddRemoteDeviceEvent_Connected value)?  connected,TResult Function( AddRemoteDeviceEvent_Rejected value)?  rejected,TResult Function( AddRemoteDeviceEvent_ConnectionFailed value)?  connectionFailed,TResult Function( AddRemoteDeviceEvent_Error value)?  error,required TResult orElse(),}){
final _that = this;
switch (_that) {
case AddRemoteDeviceEvent_WaitingForResponse() when waitingForResponse != null:
return waitingForResponse(_that);case AddRemoteDeviceEvent_Connected() when connected != null:
return connected(_that);case AddRemoteDeviceEvent_Rejected() when rejected != null:
return rejected(_that);case AddRemoteDeviceEvent_ConnectionFailed() when connectionFailed != null:
return connectionFailed(_that);case AddRemoteDeviceEvent_Error() when error != null:
return error(_that);case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// Callbacks receives the raw object, upcasted.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case final Subclass2 value:
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( AddRemoteDeviceEvent_WaitingForResponse value)  waitingForResponse,required TResult Function( AddRemoteDeviceEvent_Connected value)  connected,required TResult Function( AddRemoteDeviceEvent_Rejected value)  rejected,required TResult Function( AddRemoteDeviceEvent_ConnectionFailed value)  connectionFailed,required TResult Function( AddRemoteDeviceEvent_Error value)  error,}){
final _that = this;
switch (_that) {
case AddRemoteDeviceEvent_WaitingForResponse():
return waitingForResponse(_that);case AddRemoteDeviceEvent_Connected():
return connected(_that);case AddRemoteDeviceEvent_Rejected():
return rejected(_that);case AddRemoteDeviceEvent_ConnectionFailed():
return connectionFailed(_that);case AddRemoteDeviceEvent_Error():
return error(_that);}
}
/// A variant of `map` that fallback to returning `null`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( AddRemoteDeviceEvent_WaitingForResponse value)?  waitingForResponse,TResult? Function( AddRemoteDeviceEvent_Connected value)?  connected,TResult? Function( AddRemoteDeviceEvent_Rejected value)?  rejected,TResult? Function( AddRemoteDeviceEvent_ConnectionFailed value)?  connectionFailed,TResult? Function( AddRemoteDeviceEvent_Error value)?  error,}){
final _that = this;
switch (_that) {
case AddRemoteDeviceEvent_WaitingForResponse() when waitingForResponse != null:
return waitingForResponse(_that);case AddRemoteDeviceEvent_Connected() when connected != null:
return connected(_that);case AddRemoteDeviceEvent_Rejected() when rejected != null:
return rejected(_that);case AddRemoteDeviceEvent_ConnectionFailed() when connectionFailed != null:
return connectionFailed(_that);case AddRemoteDeviceEvent_Error() when error != null:
return error(_that);case _:
  return null;

}
}
/// A variant of `when` that fallback to an `orElse` callback.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  waitingForResponse,TResult Function()?  connected,TResult Function()?  rejected,TResult Function()?  connectionFailed,TResult Function( String field0)?  error,required TResult orElse(),}) {final _that = this;
switch (_that) {
case AddRemoteDeviceEvent_WaitingForResponse() when waitingForResponse != null:
return waitingForResponse();case AddRemoteDeviceEvent_Connected() when connected != null:
return connected();case AddRemoteDeviceEvent_Rejected() when rejected != null:
return rejected();case AddRemoteDeviceEvent_ConnectionFailed() when connectionFailed != null:
return connectionFailed();case AddRemoteDeviceEvent_Error() when error != null:
return error(_that.field0);case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// As opposed to `map`, this offers destructuring.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case Subclass2(:final field2):
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  waitingForResponse,required TResult Function()  connected,required TResult Function()  rejected,required TResult Function()  connectionFailed,required TResult Function( String field0)  error,}) {final _that = this;
switch (_that) {
case AddRemoteDeviceEvent_WaitingForResponse():
return waitingForResponse();case AddRemoteDeviceEvent_Connected():
return connected();case AddRemoteDeviceEvent_Rejected():
return rejected();case AddRemoteDeviceEvent_ConnectionFailed():
return connectionFailed();case AddRemoteDeviceEvent_Error():
return error(_that.field0);}
}
/// A variant of `when` that fallback to returning `null`
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  waitingForResponse,TResult? Function()?  connected,TResult? Function()?  rejected,TResult? Function()?  connectionFailed,TResult? Function( String field0)?  error,}) {final _that = this;
switch (_that) {
case AddRemoteDeviceEvent_WaitingForResponse() when waitingForResponse != null:
return waitingForResponse();case AddRemoteDeviceEvent_Connected() when connected != null:
return connected();case AddRemoteDeviceEvent_Rejected() when rejected != null:
return rejected();case AddRemoteDeviceEvent_ConnectionFailed() when connectionFailed != null:
return connectionFailed();case AddRemoteDeviceEvent_Error() when error != null:
return error(_that.field0);case _:
  return null;

}
}

}

/// @nodoc


class AddRemoteDeviceEvent_WaitingForResponse extends AddRemoteDeviceEvent {
  const AddRemoteDeviceEvent_WaitingForResponse(): super._();
  






@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is AddRemoteDeviceEvent_WaitingForResponse);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'AddRemoteDeviceEvent.waitingForResponse()';
}


}




/// @nodoc


class AddRemoteDeviceEvent_Connected extends AddRemoteDeviceEvent {
  const AddRemoteDeviceEvent_Connected(): super._();
  






@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is AddRemoteDeviceEvent_Connected);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'AddRemoteDeviceEvent.connected()';
}


}




/// @nodoc


class AddRemoteDeviceEvent_Rejected extends AddRemoteDeviceEvent {
  const AddRemoteDeviceEvent_Rejected(): super._();
  






@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is AddRemoteDeviceEvent_Rejected);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'AddRemoteDeviceEvent.rejected()';
}


}




/// @nodoc


class AddRemoteDeviceEvent_ConnectionFailed extends AddRemoteDeviceEvent {
  const AddRemoteDeviceEvent_ConnectionFailed(): super._();
  






@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is AddRemoteDeviceEvent_ConnectionFailed);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'AddRemoteDeviceEvent.connectionFailed()';
}


}




/// @nodoc


class AddRemoteDeviceEvent_Error extends AddRemoteDeviceEvent {
  const AddRemoteDeviceEvent_Error(this.field0): super._();
  

 final  String field0;

/// Create a copy of AddRemoteDeviceEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$AddRemoteDeviceEvent_ErrorCopyWith<AddRemoteDeviceEvent_Error> get copyWith => _$AddRemoteDeviceEvent_ErrorCopyWithImpl<AddRemoteDeviceEvent_Error>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is AddRemoteDeviceEvent_Error&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'AddRemoteDeviceEvent.error(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $AddRemoteDeviceEvent_ErrorCopyWith<$Res> implements $AddRemoteDeviceEventCopyWith<$Res> {
  factory $AddRemoteDeviceEvent_ErrorCopyWith(AddRemoteDeviceEvent_Error value, $Res Function(AddRemoteDeviceEvent_Error) _then) = _$AddRemoteDeviceEvent_ErrorCopyWithImpl;
@useResult
$Res call({
 String field0
});




}
/// @nodoc
class _$AddRemoteDeviceEvent_ErrorCopyWithImpl<$Res>
    implements $AddRemoteDeviceEvent_ErrorCopyWith<$Res> {
  _$AddRemoteDeviceEvent_ErrorCopyWithImpl(this._self, this._then);

  final AddRemoteDeviceEvent_Error _self;
  final $Res Function(AddRemoteDeviceEvent_Error) _then;

/// Create a copy of AddRemoteDeviceEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(AddRemoteDeviceEvent_Error(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

// dart format on
